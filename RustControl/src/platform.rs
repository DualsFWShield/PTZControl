//! Windows platform glue: single instance, IPC and system-wide hotkeys.

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use windows::core::w;
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, FindWindowW, GetMessageW,
    GetWindowLongPtrW, PostMessageW, PostQuitMessage, RegisterClassW, SendMessageW,
    SetWindowLongPtrW, TranslateMessage, GWLP_USERDATA, MSG, WINDOW_EX_STYLE, WM_APP, WM_CLOSE,
    WM_COPYDATA, WM_DESTROY, WM_HOTKEY, WNDCLASSW, WS_POPUP,
};

use crate::cli::{self, RemoteMessage};
use crate::config::{HotkeyBinding, HotkeyId};
use crate::hotkeys::{key_to_vk, modifier_bits};

const WINDOW_CLASS: windows::core::PCWSTR = w!("PTZControlRustHiddenWindow");
const WINDOW_TITLE: windows::core::PCWSTR = w!("PTZControlRust");
const WM_RELOAD_HOTKEYS: u32 = WM_APP + 1;

#[derive(Debug)]
pub enum PlatformEvent {
    Hotkey(HotkeyId),
    Message(RemoteMessage),
    HotkeyError {
        target: HotkeyId,
        message: String,
    },
}

struct RegisteredHotkey {
    id: i32,
    target: HotkeyId,
}

struct PlatformState {
    tx: Sender<PlatformEvent>,
    shared: Arc<Mutex<Vec<HotkeyBinding>>>,
    registered: Vec<RegisteredHotkey>,
    next_id: i32,
}

pub struct Platform {
    hwnd: isize,
    shared: Arc<Mutex<Vec<HotkeyBinding>>>,
    thread: Option<JoinHandle<()>>,
}

impl Platform {
    pub fn start() -> Result<(Self, Receiver<PlatformEvent>), String> {
        let (tx, rx) = mpsc::channel();
        let shared = Arc::new(Mutex::new(Vec::new()));
        let (ready_tx, ready_rx) = mpsc::channel::<Result<isize, String>>();
        let thread_shared = shared.clone();

        let join = thread::Builder::new()
            .name("ptzcontrol-platform".into())
            .spawn(move || {
                let state = Box::new(PlatformState {
                    tx,
                    shared: thread_shared,
                    registered: Vec::new(),
                    next_id: 1,
                });
                let state_ptr = Box::into_raw(state);
                unsafe {
                    let hinstance = match GetModuleHandleW(None) {
                        Ok(module) => module,
                        Err(err) => {
                            let _ = ready_tx.send(Err(err.to_string()));
                            drop(Box::from_raw(state_ptr));
                            return;
                        }
                    };
                    let class = WNDCLASSW {
                        style: Default::default(),
                        lpfnWndProc: Some(wnd_proc),
                        cbClsExtra: 0,
                        cbWndExtra: 0,
                        hInstance: windows::Win32::Foundation::HINSTANCE(hinstance.0),
                        hIcon: Default::default(),
                        hCursor: Default::default(),
                        hbrBackground: Default::default(),
                        lpszMenuName: windows::core::PCWSTR::null(),
                        lpszClassName: WINDOW_CLASS,
                    };
                    if RegisterClassW(&class) == 0 {
                        let _ = ready_tx.send(Err("Impossible d'enregistrer la fenêtre d'IPC".into()));
                        drop(Box::from_raw(state_ptr));
                        return;
                    }
                    let hwnd = match CreateWindowExW(
                        WINDOW_EX_STYLE(0),
                        WINDOW_CLASS,
                        WINDOW_TITLE,
                        WS_POPUP,
                        0,
                        0,
                        0,
                        0,
                        None,
                        None,
                        windows::Win32::Foundation::HINSTANCE(hinstance.0),
                        None,
                    ) {
                        Ok(hwnd) => hwnd,
                        Err(err) => {
                            let _ = ready_tx.send(Err(format!("Fenêtre IPC: {err}")));
                            drop(Box::from_raw(state_ptr));
                            return;
                        }
                    };
                    set_window_user_data(hwnd, state_ptr);
                    let _ = ready_tx.send(Ok(hwnd.0 as isize));

                    let mut msg = MSG::default();
                    loop {
                        let ret = GetMessageW(&mut msg, None, 0, 0);
                        if ret.0 == 0 || ret.0 == -1 {
                            break;
                        }
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                    drop(Box::from_raw(state_ptr));
                }
            })
            .map_err(|err| format!("Impossible de démarrer le thread système: {err}"))?;

        let hwnd = match ready_rx.recv_timeout(Duration::from_secs(3)) {
            Ok(Ok(hwnd)) => hwnd,
            Ok(Err(err)) => {
                let _ = join.join();
                return Err(err);
            }
            Err(_) => {
                let _ = join.join();
                return Err("La fenêtre système n'a pas répondu".into());
            }
        };

        Ok((
            Self {
                hwnd,
                shared,
                thread: Some(join),
            },
            rx,
        ))
    }

    pub fn reload_hotkeys(&self, bindings: Vec<HotkeyBinding>) {
        if let Ok(mut shared) = self.shared.lock() {
            *shared = bindings;
        }
        unsafe {
            let _ = PostMessageW(
                HWND(self.hwnd as *mut core::ffi::c_void),
                WM_RELOAD_HOTKEYS,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
}

impl Drop for Platform {
    fn drop(&mut self) {
        unsafe {
            let _ = PostMessageW(
                HWND(self.hwnd as *mut core::ffi::c_void),
                WM_CLOSE,
                WPARAM(0),
                LPARAM(0),
            );
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[inline]
unsafe fn set_window_user_data(hwnd: HWND, ptr: *mut PlatformState) {
    #[cfg(target_pointer_width = "64")]
    SetWindowLongPtrW(hwnd, GWLP_USERDATA, ptr as isize);
    #[cfg(target_pointer_width = "32")]
    SetWindowLongPtrW(hwnd, GWLP_USERDATA, ptr as i32);
}

#[inline]
unsafe fn get_window_user_data(hwnd: HWND) -> *mut PlatformState {
    #[cfg(target_pointer_width = "64")]
    {
        GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut PlatformState
    }
    #[cfg(target_pointer_width = "32")]
    {
        GetWindowLongPtrW(hwnd, GWLP_USERDATA) as usize as *mut PlatformState
    }
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_HOTKEY => {
            let state_ptr = get_window_user_data(hwnd);
            if !state_ptr.is_null() {
                let state = &*state_ptr;
                let id = wparam.0 as i32;
                if let Some(registered) = state.registered.iter().find(|r| r.id == id) {
                    let _ = state.tx.send(PlatformEvent::Hotkey(registered.target));
                }
            }
            LRESULT(0)
        }
        WM_COPYDATA => {
            let cds = &*(lparam.0 as *const COPYDATASTRUCT);
            if !cds.lpData.is_null() && cds.cbData > 0 {
                let bytes = std::slice::from_raw_parts(cds.lpData as *const u8, cds.cbData as usize);
                if let Some(message) = cli::decode(bytes) {
                    let state_ptr = get_window_user_data(hwnd);
                    if !state_ptr.is_null() {
                        let state = &*state_ptr;
                        let _ = state.tx.send(PlatformEvent::Message(message));
                    }
                }
            }
            LRESULT(1)
        }
        WM_RELOAD_HOTKEYS => {
            let state_ptr = get_window_user_data(hwnd);
            if !state_ptr.is_null() {
                reload_hotkeys(&mut *state_ptr, hwnd);
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn reload_hotkeys(state: &mut PlatformState, hwnd: HWND) {
    for registered in state.registered.iter() {
        let _ = UnregisterHotKey(hwnd, registered.id);
    }
    state.registered.clear();

    let bindings = match state.shared.lock() {
        Ok(shared) => shared.clone(),
        Err(_) => return,
    };

    for binding in bindings {
        let Some(ref hotkey) = binding.hotkey else {
            continue;
        };
        let Some(vk) = key_to_vk(&hotkey.key) else {
            let _ = state.tx.send(PlatformEvent::HotkeyError {
                target: binding.id(),
                message: format!("Touche inconnue: {}", hotkey.key),
            });
            continue;
        };
        let id = state.next_id;
        state.next_id += 1;
        let modifiers = HOT_KEY_MODIFIERS(modifier_bits(&hotkey));
        match RegisterHotKey(hwnd, id, modifiers, vk) {
            Ok(()) => state.registered.push(RegisteredHotkey {
                id,
                target: binding.id(),
            }),
            Err(err) => {
                let _ = state.tx.send(PlatformEvent::HotkeyError {
                    target: binding.id(),
                    message: format!("{} ({})", err, hotkey.label()),
                });
            }
        }
    }
}

pub struct InstanceGuard {
    handle: isize,
}

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(HANDLE(self.handle as *mut core::ffi::c_void));
        }
    }
}

/// Returns `(guard, already_running)`.
pub fn acquire_single_instance() -> Result<(InstanceGuard, bool), String> {
    unsafe {
        let handle = CreateMutexW(None, false, w!("Local\\PTZControlRustInstance"))
            .map_err(|err| err.to_string())?;
        let already_running = GetLastError() == ERROR_ALREADY_EXISTS;
        Ok((
            InstanceGuard {
                handle: handle.0 as isize,
            },
            already_running,
        ))
    }
}

pub fn send_to_running_instance(message: &RemoteMessage) -> bool {
    let bytes = cli::encode(message);
    let data = COPYDATASTRUCT {
        dwData: 1,
        cbData: bytes.len() as u32,
        lpData: bytes.as_ptr() as *mut core::ffi::c_void,
    };
    unsafe {
        let Ok(hwnd) = FindWindowW(WINDOW_CLASS, WINDOW_TITLE) else {
            return false;
        };
        let result = SendMessageW(
            hwnd,
            WM_COPYDATA,
            WPARAM(0),
            LPARAM(&data as *const COPYDATASTRUCT as isize),
        );
        result.0 == 1
    }
}
