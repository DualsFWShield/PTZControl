//! DirectShow + Logitech extension-unit camera backend.
//!
//! * `IAMCameraControl` provides pan/tilt/zoom/focus.
//! * `IAMVideoProcAmp` provides brightness/contrast/saturation.
//! * `IKsControl`/`IKsTopologyInfo` gives access to Logitech's CSRK extension
//!   units, used for hardware presets and the smoother motor control.

use std::ffi::c_void;
use std::fmt;
use std::mem::size_of;
use std::thread::sleep;
use std::time::Duration;

use windows::core::{w, GUID, Interface, VARIANT};
use windows::Win32::Foundation::S_OK;
use windows::Win32::Media::DirectShow::{
    CameraControl_Flags_Auto, CameraControl_Flags_Manual, CameraControl_Focus, CameraControl_Pan,
    CameraControl_Tilt, CameraControl_Zoom, IAMCameraControl, IAMVideoProcAmp, ICreateDevEnum,
    VideoProcAmp_Brightness, VideoProcAmp_Contrast, VideoProcAmp_Saturation,
};
use windows::Win32::Media::KernelStreaming::{
    IKsControl, IKsTopologyInfo, KSIDENTIFIER, KSIDENTIFIER_0, KSIDENTIFIER_0_0, KSNODETYPE_DEV_SPECIFIC,
    KSPROPERTY_TYPE_SET, KSPROPERTY_TYPE_SETSUPPORT, KSPROPERTY_TYPE_TOPOLOGY,
    KSP_NODE,
};
use windows::Win32::Media::MediaFoundation::{
    CLSID_SystemDeviceEnum, CLSID_VideoInputDeviceCategory,
};
use windows::Win32::System::Com::StructuredStorage::IPropertyBag;
use windows::Win32::System::Com::{CoCreateInstance, IEnumMoniker, IMoniker, CLSCTX_INPROC_SERVER};

pub const NUM_PRESETS: usize = 8;

// Logitech CSRK property sets from the original application.
const XU_DEVICE_INFORMATION: GUID = GUID::from_u128(0x69678ee4_410f_40db_a850_7420d7d8240e);
const XU_VIDEOPIPE_CONTROL: GUID = GUID::from_u128(0x49e40215_f434_47fe_b158_0e885023e51b);
const XU_TEST_DEBUG: GUID = GUID::from_u128(0x1f5d4ca9_de11_4487_840d_50933c8ec8d1);
const XU_PERIPHERAL_CONTROL: GUID = GUID::from_u128(0xffe52d21_8030_4e2c_82d9_f587d00540bd);

const XU_VIDEO_FW_ZOOM_CONTROL: u32 = 0x06;
const XU_PERIPHERAL_PANTILT_RELATIVE_CONTROL: u32 = 0x01;
const XU_PERIPHERAL_PANTILT_MODE_CONTROL: u32 = 0x02;

const PANTILT_RESET_NOACTION: u32 = 0;
const PANTILT_SET_PRESET_BASE: u32 = 4;
const PANTILT_GOTO_PRESET_BASE: u32 = 12;
const PANTILT_GOTOHOME: u32 = 3;

const KSPROPERTY_CAMERACONTROL_PAN_RELATIVE: i32 = 10;
const KSPROPERTY_CAMERACONTROL_TILT_RELATIVE: i32 = 11;

#[derive(Clone, Debug)]
pub struct CameraError(pub String);

impl fmt::Display for CameraError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for CameraError {}

impl From<windows::core::Error> for CameraError {
    fn from(value: windows::core::Error) -> Self {
        Self(value.to_string())
    }
}

type Result<T> = std::result::Result<T, CameraError>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceInfo {
    pub name: String,
    pub path: String,
    pub vid: u16,
    pub pid: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range {
    pub min: i32,
    pub max: i32,
    pub step: i32,
    pub default: i32,
}

impl Range {
    pub fn midpoint(self) -> i32 {
        self.min + (self.max - self.min) / 2
    }

    pub fn clamp(self, value: i32) -> i32 {
        value.clamp(self.min, self.max)
    }

    /// The step used by the tiny incremental zoom command.
    pub fn zoom_delta(self) -> i32 {
        let unit = if self.step > 0 { self.step } else { 1 };
        (((self.max - self.min) / 150).max(1)) * unit
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct XuNodes {
    pub device_information: Option<u32>,
    pub video_pipe: Option<u32>,
    pub test_debug: Option<u32>,
    pub peripheral: Option<u32>,
}

pub fn list_devices() -> Result<Vec<DeviceInfo>> {
    unsafe {
        let enumerator: ICreateDevEnum =
            CoCreateInstance(&CLSID_SystemDeviceEnum, None, CLSCTX_INPROC_SERVER)?;
        let mut category: Option<IEnumMoniker> = None;
        enumerator.CreateClassEnumerator(&CLSID_VideoInputDeviceCategory, &mut category, 0)?;
        let Some(category) = category else {
            return Ok(Vec::new());
        };

        let mut devices = Vec::new();
        loop {
            let mut monikers = [None];
            let hr = category.Next(&mut monikers, None);
            if hr != S_OK {
                break;
            }
            let Some(moniker) = monikers[0].take() else {
                break;
            };
            if let Some(device) = device_info(&moniker) {
                devices.push(device);
            }
        }
        Ok(devices)
    }
}

unsafe fn device_info(moniker: &IMoniker) -> Option<DeviceInfo> {
    let bag: IPropertyBag = moniker.BindToStorage(None, None).ok()?;

    let mut name = VARIANT::default();
    bag.Read(w!("FriendlyName"), &mut name, None).ok()?;
    let name = name.to_string();

    let mut path = VARIANT::default();
    bag.Read(w!("DevicePath"), &mut path, None).ok()?;
    let path = path.to_string();
    let (vid, pid) = parse_vid_pid(&path);

    Some(DeviceInfo {
        name,
        path,
        vid,
        pid,
    })
}

fn parse_vid_pid(path: &str) -> (u16, u16) {
    let lower = path.to_ascii_lowercase();
    let parse_after = |needle: &str| -> u16 {
        lower
            .find(needle)
            .and_then(|index| lower.get(index + needle.len()..index + needle.len() + 4))
            .and_then(|hex| u16::from_str_radix(hex, 16).ok())
            .unwrap_or(0)
    };
    (parse_after("vid_"), parse_after("pid_"))
}

fn find_moniker(path: &str) -> Result<IMoniker> {
    unsafe {
        let enumerator: ICreateDevEnum =
            CoCreateInstance(&CLSID_SystemDeviceEnum, None, CLSCTX_INPROC_SERVER)?;
        let mut category: Option<IEnumMoniker> = None;
        enumerator.CreateClassEnumerator(&CLSID_VideoInputDeviceCategory, &mut category, 0)?;
        let Some(category) = category else {
            return Err(CameraError("Aucune caméra détectée".into()));
        };
        loop {
            let mut monikers = [None];
            let hr = category.Next(&mut monikers, None);
            if hr != S_OK {
                break;
            }
            let Some(moniker) = monikers[0].take() else {
                break;
            };
            let bag: IPropertyBag = match moniker.BindToStorage(None, None) {
                Ok(bag) => bag,
                Err(_) => continue,
            };
            let mut variant = VARIANT::default();
            if bag.Read(w!("DevicePath"), &mut variant, None).is_ok() {
                let candidate = variant.to_string();
                if candidate.eq_ignore_ascii_case(path) {
                    return Ok(moniker);
                }
            }
        }
    }
    Err(CameraError(format!(
        "La caméra « {path} » n'est plus disponible"
    )))
}

pub struct Camera {
    info: DeviceInfo,
    ks: IKsControl,
    camera: Option<IAMCameraControl>,
    proc_amp: Option<IAMVideoProcAmp>,
    nodes: XuNodes,
    mechanical_pan_tilt: bool,
    use_logitech_motion: bool,
    motor_interval: Duration,
    pub pan_range: Option<Range>,
    pub tilt_range: Option<Range>,
    pub zoom_range: Option<Range>,
    pub focus_range: Option<Range>,
    pub brightness_range: Option<Range>,
    pub contrast_range: Option<Range>,
    pub saturation_range: Option<Range>,
}

impl Camera {
    pub fn open(
        info: &DeviceInfo,
        use_logitech_motion: bool,
        motor_interval_ms: u32,
    ) -> Result<Self> {
        let moniker = find_moniker(&info.path)?;
        unsafe {
            let ks: IKsControl = moniker.BindToObject(None, None)?;
            let nodes = discover_xu_nodes(&ks);
            let camera = ks.cast::<IAMCameraControl>().ok();
            let proc_amp = ks.cast::<IAMVideoProcAmp>().ok();

            let mechanical_pan_tilt = camera
                .as_ref()
                .map(|cam| {
                    let mut value = 0;
                    let mut flags = 0;
                    cam.Get(KSPROPERTY_CAMERACONTROL_PAN_RELATIVE, &mut value, &mut flags)
                        .is_ok()
                })
                .unwrap_or(false);

            let pan_range = camera.as_ref().and_then(|cam| camera_range(cam, CameraControl_Pan.0));
            let tilt_range =
                camera.as_ref().and_then(|cam| camera_range(cam, CameraControl_Tilt.0));
            let zoom_range =
                camera.as_ref().and_then(|cam| camera_range(cam, CameraControl_Zoom.0));
            let focus_range =
                camera.as_ref().and_then(|cam| camera_range(cam, CameraControl_Focus.0));
            let brightness_range = proc_amp
                .as_ref()
                .and_then(|amp| proc_range(amp, VideoProcAmp_Brightness.0));
            let contrast_range = proc_amp
                .as_ref()
                .and_then(|amp| proc_range(amp, VideoProcAmp_Contrast.0));
            let saturation_range = proc_amp
                .as_ref()
                .and_then(|amp| proc_range(amp, VideoProcAmp_Saturation.0));

            Ok(Self {
                info: info.clone(),
                ks,
                camera,
                proc_amp,
                nodes,
                mechanical_pan_tilt,
                use_logitech_motion,
                motor_interval: Duration::from_millis(motor_interval_ms.clamp(10, 1000) as u64),
                pan_range,
                tilt_range,
                zoom_range,
                focus_range,
                brightness_range,
                contrast_range,
                saturation_range,
            })
        }
    }

    pub fn info(&self) -> &DeviceInfo {
        &self.info
    }

    pub fn set_motion_options(&mut self, use_logitech_motion: bool, motor_interval_ms: u32) {
        self.use_logitech_motion = use_logitech_motion;
        self.motor_interval = Duration::from_millis(motor_interval_ms.clamp(10, 1000) as u64);
    }

    pub fn has_hardware_presets(&self) -> bool {
        self.nodes.peripheral.is_some()
    }

    pub fn motion_mode_label(&self) -> &'static str {
        if self.use_logitech_motion && self.nodes.peripheral.is_some() {
            "Moteurs Logitech (XU)"
        } else if self.mechanical_pan_tilt {
            "DirectShow relatif"
        } else {
            "DirectShow absolu"
        }
    }

    pub fn pan(&self, direction: i32) -> Result<()> {
        if direction == 0 {
            return self.stop_motion();
        }
        let direction = direction.signum();
        if self.use_logitech_motion {
            if let Some(node) = self.nodes.peripheral {
                // A short velocity pulse, mirroring the motor interval used by
                // the DirectShow path. Without the trailing zero the Logitech
                // motor would keep moving after a single hotkey press.
                let value = ((direction as i8 as u8) as u32) << 8;
                self.set_xu(
                    XU_PERIPHERAL_CONTROL,
                    node,
                    XU_PERIPHERAL_PANTILT_RELATIVE_CONTROL,
                    value,
                )?;
                sleep(self.motor_interval);
                return self.set_xu(
                    XU_PERIPHERAL_CONTROL,
                    node,
                    XU_PERIPHERAL_PANTILT_RELATIVE_CONTROL,
                    0,
                );
            }
        }
        let Some(camera) = &self.camera else {
            return Ok(());
        };
        unsafe {
            if self.mechanical_pan_tilt {
                camera.Set(KSPROPERTY_CAMERACONTROL_PAN_RELATIVE, direction, 0)?;
                sleep(self.motor_interval);
                camera.Set(KSPROPERTY_CAMERACONTROL_PAN_RELATIVE, 0, 0)?;
            } else if let Some(range) = self.pan_range {
                let mut value = 0;
                let mut flags = 0;
                if camera.Get(CameraControl_Pan.0, &mut value, &mut flags).is_ok() {
                    value = range.clamp(value + direction);
                    camera.Set(CameraControl_Pan.0, value, flags)?;
                }
            }
        }
        Ok(())
    }

    pub fn tilt(&self, direction: i32) -> Result<()> {
        if direction == 0 {
            return self.stop_motion();
        }
        let direction = direction.signum();
        if self.use_logitech_motion {
            if let Some(node) = self.nodes.peripheral {
                // The Logitech tilt axis uses the opposite sign of the
                // DirectShow camera-control value.
                let value = (((-direction as i8 as u8) as u32) << 8) << 16;
                self.set_xu(
                    XU_PERIPHERAL_CONTROL,
                    node,
                    XU_PERIPHERAL_PANTILT_RELATIVE_CONTROL,
                    value,
                )?;
                sleep(self.motor_interval);
                return self.set_xu(
                    XU_PERIPHERAL_CONTROL,
                    node,
                    XU_PERIPHERAL_PANTILT_RELATIVE_CONTROL,
                    0,
                );
            }
        }
        let Some(camera) = &self.camera else {
            return Ok(());
        };
        unsafe {
            if self.mechanical_pan_tilt {
                camera.Set(KSPROPERTY_CAMERACONTROL_TILT_RELATIVE, direction, 0)?;
                sleep(self.motor_interval);
                camera.Set(KSPROPERTY_CAMERACONTROL_TILT_RELATIVE, 0, 0)?;
            } else if let Some(range) = self.tilt_range {
                let mut value = 0;
                let mut flags = 0;
                if camera.Get(CameraControl_Tilt.0, &mut value, &mut flags).is_ok() {
                    value = range.clamp(value + direction);
                    camera.Set(CameraControl_Tilt.0, value, flags)?;
                }
            }
        }
        Ok(())
    }

    pub fn stop_motion(&self) -> Result<()> {
        if self.use_logitech_motion {
            if let Some(node) = self.nodes.peripheral {
                self.set_xu(
                    XU_PERIPHERAL_CONTROL,
                    node,
                    XU_PERIPHERAL_PANTILT_RELATIVE_CONTROL,
                    PANTILT_RESET_NOACTION,
                )?;
            }
        }
        if let Some(camera) = &self.camera {
            unsafe {
                let _ = camera.Set(KSPROPERTY_CAMERACONTROL_PAN_RELATIVE, 0, 0);
                let _ = camera.Set(KSPROPERTY_CAMERACONTROL_TILT_RELATIVE, 0, 0);
            }
        }
        Ok(())
    }

    pub fn zoom(&self, direction: i32) -> Result<()> {
        let (Some(camera), Some(range)) = (&self.camera, self.zoom_range) else {
            return Ok(());
        };
        unsafe {
            let mut value = range.default;
            let mut flags = CameraControl_Flags_Manual.0;
            let _ = camera.Get(CameraControl_Zoom.0, &mut value, &mut flags);
            if value < range.min || value > range.max {
                value = range.default;
            }
            let next = range.clamp(value + direction.signum() * range.zoom_delta());
            camera.Set(CameraControl_Zoom.0, next, CameraControl_Flags_Manual.0)?;
        }
        Ok(())
    }

    pub fn focus(&self) -> Option<(i32, bool)> {
        let camera = self.camera.as_ref()?;
        unsafe {
            let mut value = 0;
            let mut flags = 0;
            camera.Get(CameraControl_Focus.0, &mut value, &mut flags).ok()?;
            Some((value, flags & CameraControl_Flags_Auto.0 != 0))
        }
    }

    pub fn set_focus(&self, value: i32, auto: bool) -> Result<()> {
        let Some(camera) = &self.camera else {
            return Err(CameraError("Cette caméra ne gère pas la mise au point".into()));
        };
        let value = self
            .focus_range
            .map(|range| range.clamp(value))
            .unwrap_or(value);
        let flags = if auto {
            CameraControl_Flags_Auto.0
        } else {
            CameraControl_Flags_Manual.0
        };
        unsafe { camera.Set(CameraControl_Focus.0, value, flags)? };
        Ok(())
    }

    pub fn brightness(&self) -> Option<i32> {
        let amp = self.proc_amp.as_ref()?;
        unsafe {
            let mut value = 0;
            let mut flags = 0;
            amp.Get(VideoProcAmp_Brightness.0, &mut value, &mut flags).ok()?;
            Some(value)
        }
    }

    pub fn set_brightness(&self, value: i32) -> Result<()> {
        let Some(amp) = &self.proc_amp else {
            return Ok(());
        };
        let value = self
            .brightness_range
            .map(|range| range.clamp(value))
            .unwrap_or(value);
        unsafe { amp.Set(VideoProcAmp_Brightness.0, value, 1)? };
        Ok(())
    }

    pub fn contrast(&self) -> Option<i32> {
        let amp = self.proc_amp.as_ref()?;
        unsafe {
            let mut value = 0;
            let mut flags = 0;
            amp.Get(VideoProcAmp_Contrast.0, &mut value, &mut flags).ok()?;
            Some(value)
        }
    }

    pub fn set_contrast(&self, value: i32) -> Result<()> {
        let Some(amp) = &self.proc_amp else {
            return Ok(());
        };
        let value = self
            .contrast_range
            .map(|range| range.clamp(value))
            .unwrap_or(value);
        unsafe { amp.Set(VideoProcAmp_Contrast.0, value, 1)? };
        Ok(())
    }

    pub fn saturation(&self) -> Option<i32> {
        let amp = self.proc_amp.as_ref()?;
        unsafe {
            let mut value = 0;
            let mut flags = 0;
            amp.Get(VideoProcAmp_Saturation.0, &mut value, &mut flags).ok()?;
            Some(value)
        }
    }

    pub fn set_saturation(&self, value: i32) -> Result<()> {
        let Some(amp) = &self.proc_amp else {
            return Ok(());
        };
        let value = self
            .saturation_range
            .map(|range| range.clamp(value))
            .unwrap_or(value);
        unsafe { amp.Set(VideoProcAmp_Saturation.0, value, 1)? };
        Ok(())
    }

    pub fn home(&self) -> Result<()> {
        if let Some(node) = self.nodes.video_pipe {
            let value: u32 = 0;
            let _ = self.set_xu(XU_VIDEOPIPE_CONTROL, node, XU_VIDEO_FW_ZOOM_CONTROL, value);
        }
        if let Some(node) = self.nodes.peripheral {
            self.set_xu(
                XU_PERIPHERAL_CONTROL,
                node,
                XU_PERIPHERAL_PANTILT_MODE_CONTROL,
                PANTILT_GOTOHOME,
            )?;
        } else if let Some(range) = self.zoom_range {
            let _ = self.zoom_to(range.default);
        }
        Ok(())
    }

    fn zoom_to(&self, value: i32) -> Result<()> {
        if let Some(camera) = &self.camera {
            unsafe {
                camera.Set(CameraControl_Zoom.0, value, CameraControl_Flags_Manual.0)?;
            }
        }
        Ok(())
    }

    pub fn save_preset(&self, slot: usize) -> Result<()> {
        if slot >= NUM_PRESETS {
            return Err(CameraError("Numéro de préréglage invalide".into()));
        }
        let Some(node) = self.nodes.peripheral else {
            return Err(CameraError(
                "Cette caméra ne stocke pas les préréglages dans son matériel".into(),
            ));
        };
        self.set_xu(
            XU_PERIPHERAL_CONTROL,
            node,
            XU_PERIPHERAL_PANTILT_MODE_CONTROL,
            PANTILT_SET_PRESET_BASE + slot as u32,
        )
    }

    pub fn recall_preset(&self, slot: usize) -> Result<()> {
        if slot >= NUM_PRESETS {
            return Err(CameraError("Numéro de préréglage invalide".into()));
        }
        let Some(node) = self.nodes.peripheral else {
            return Err(CameraError(
                "Cette caméra ne gère pas les préréglages matériels".into(),
            ));
        };
        self.set_xu(
            XU_PERIPHERAL_CONTROL,
            node,
            XU_PERIPHERAL_PANTILT_MODE_CONTROL,
            PANTILT_GOTO_PRESET_BASE + slot as u32,
        )
    }

    fn set_xu(&self, set: GUID, node: u32, id: u32, value: u32) -> Result<()> {
        unsafe {
            let node = make_node(set, node, id, KSPROPERTY_TYPE_SET | KSPROPERTY_TYPE_TOPOLOGY);
            let mut bytes = 0;
            self.ks.KsProperty(
                &node as *const KSP_NODE as *const KSIDENTIFIER,
                size_of::<KSP_NODE>() as u32,
                &value as *const u32 as *mut c_void,
                size_of::<u32>() as u32,
                &mut bytes,
            )?;
        }
        Ok(())
    }
}

fn camera_range(camera: &IAMCameraControl, property: i32) -> Option<Range> {
    unsafe {
        let mut min = 0;
        let mut max = 0;
        let mut step = 0;
        let mut default = 0;
        let mut flags = 0;
        camera
            .GetRange(property, &mut min, &mut max, &mut step, &mut default, &mut flags)
            .ok()?;
        Some(Range {
            min,
            max,
            step,
            default,
        })
    }
}

fn proc_range(amp: &IAMVideoProcAmp, property: i32) -> Option<Range> {
    unsafe {
        let mut min = 0;
        let mut max = 0;
        let mut step = 0;
        let mut default = 0;
        let mut flags = 0;
        amp.GetRange(property, &mut min, &mut max, &mut step, &mut default, &mut flags)
            .ok()?;
        Some(Range {
            min,
            max,
            step,
            default,
        })
    }
}

fn make_node(set: GUID, node_id: u32, id: u32, flags: u32) -> KSP_NODE {
    KSP_NODE {
        Property: KSIDENTIFIER {
            Anonymous: KSIDENTIFIER_0 {
                Anonymous: KSIDENTIFIER_0_0 { Set: set, Id: id, Flags: flags },
            },
        },
        NodeId: node_id,
        Reserved: 0,
    }
}

fn discover_xu_nodes(ks: &IKsControl) -> XuNodes {
    let mut nodes = XuNodes::default();
    unsafe {
        let Ok(topology) = ks.cast::<IKsTopologyInfo>() else {
            return nodes;
        };
        let Ok(count) = topology.NumNodes() else {
            return nodes;
        };
        for node in 0..count {
            let Ok(kind) = topology.get_NodeType(node) else {
                continue;
            };
            if kind != KSNODETYPE_DEV_SPECIFIC {
                continue;
            }
            if nodes.device_information.is_none() && supports(ks, XU_DEVICE_INFORMATION, node) {
                nodes.device_information = Some(node);
            }
            if nodes.video_pipe.is_none() && supports(ks, XU_VIDEOPIPE_CONTROL, node) {
                nodes.video_pipe = Some(node);
            }
            if nodes.test_debug.is_none() && supports(ks, XU_TEST_DEBUG, node) {
                nodes.test_debug = Some(node);
            }
            if nodes.peripheral.is_none() && supports(ks, XU_PERIPHERAL_CONTROL, node) {
                nodes.peripheral = Some(node);
            }
        }
    }
    nodes
}

fn supports(ks: &IKsControl, set: GUID, node_id: u32) -> bool {
    unsafe {
        let node = make_node(
            set,
            node_id,
            0,
            KSPROPERTY_TYPE_SETSUPPORT | KSPROPERTY_TYPE_TOPOLOGY,
        );
        let mut bytes = 0;
        ks.KsProperty(
            &node as *const KSP_NODE as *const KSIDENTIFIER,
            size_of::<KSP_NODE>() as u32,
            std::ptr::null_mut(),
            0,
            &mut bytes,
        )
        .is_ok()
    }
}
