// PTZControl
// Copyright (C) 2026 Martin Richter (xMRi-Software) - webmaster@m-ri.de
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.
// See the GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see
// <https://www.gnu.org/licenses/>.
// 
// SPDX-License-Identifier: GPL-3.0-or-later

// SettingsDlg.cpp : implementation file
//

#include "pch.h"
#include "FileVersionInfo.h"
#include "PTZControl.h"
#include "SettingsDlg.h"
#include "afxdialogex.h"


// CSettingsDlg dialog

IMPLEMENT_DYNAMIC(CSettingsDlg, CDialogEx)

CSettingsDlg::CSettingsDlg(CWnd* pParent /*=nullptr*/)
	: CDialogEx(IDD_SETTINGS, pParent)
	, m_bLogitechCameraControl(FALSE)
	, m_iMotorIntervalTimer(0)
	, m_pCurrentWebCam(nullptr)
{

}

CSettingsDlg::~CSettingsDlg()
{
}

void CSettingsDlg::DoDataExchange(CDataExchange* pDX)
{
	CDialogEx::DoDataExchange(pDX);
	DDX_Text(pDX, IDC_ED_CAMERA, m_strCameraName);
	DDX_Check(pDX, IDC_CH_LOGITECHCONTROL, m_bLogitechCameraControl);
	DDX_Text(pDX, IDC_ED_MOTORTIME, m_iMotorIntervalTimer);
	DDX_Control(pDX, IDC_ED_MOTORTIME, m_edMotorInterval);
	DDX_Control(pDX, IDC_CH_LOGITECHCONTROL, m_chLogitechControl);
	
	// Tooltips
	DDX_Text(pDX, IDC_ED_TOOLTIP_1_1, m_strTooltip[0][0]);
	DDX_Text(pDX, IDC_ED_TOOLTIP_1_2, m_strTooltip[0][1]);
	DDX_Text(pDX, IDC_ED_TOOLTIP_1_3, m_strTooltip[0][2]);
	DDX_Text(pDX, IDC_ED_TOOLTIP_1_4, m_strTooltip[0][3]);
	DDX_Text(pDX, IDC_ED_TOOLTIP_1_5, m_strTooltip[0][4]);
	DDX_Text(pDX, IDC_ED_TOOLTIP_1_6, m_strTooltip[0][5]);
	DDX_Text(pDX, IDC_ED_TOOLTIP_1_7, m_strTooltip[0][6]);
	DDX_Text(pDX, IDC_ED_TOOLTIP_1_8, m_strTooltip[0][7]);
	DDX_Text(pDX, IDC_ED_TOOLTIP_2_1, m_strTooltip[1][0]);
	DDX_Text(pDX, IDC_ED_TOOLTIP_2_2, m_strTooltip[1][1]);
	DDX_Text(pDX, IDC_ED_TOOLTIP_2_3, m_strTooltip[1][2]);
	DDX_Text(pDX, IDC_ED_TOOLTIP_2_4, m_strTooltip[1][3]);
	DDX_Text(pDX, IDC_ED_TOOLTIP_2_5, m_strTooltip[1][4]);
	DDX_Text(pDX, IDC_ED_TOOLTIP_2_6, m_strTooltip[1][5]);
	DDX_Text(pDX, IDC_ED_TOOLTIP_2_7, m_strTooltip[1][6]);
	DDX_Text(pDX, IDC_ED_TOOLTIP_2_8, m_strTooltip[1][7]);
	DDX_Text(pDX, IDC_ED_TOOLTIP_3_1, m_strTooltip[2][0]);
	DDX_Text(pDX, IDC_ED_TOOLTIP_3_2, m_strTooltip[2][1]);
	DDX_Text(pDX, IDC_ED_TOOLTIP_3_3, m_strTooltip[2][2]);
	DDX_Text(pDX, IDC_ED_TOOLTIP_3_4, m_strTooltip[2][3]);
	DDX_Text(pDX, IDC_ED_TOOLTIP_3_5, m_strTooltip[2][4]);
	DDX_Text(pDX, IDC_ED_TOOLTIP_3_6, m_strTooltip[2][5]);
	DDX_Text(pDX, IDC_ED_TOOLTIP_3_7, m_strTooltip[2][6]);
	DDX_Text(pDX, IDC_ED_TOOLTIP_3_8, m_strTooltip[2][7]);

	// Image and Optical controls
	DDX_Control(pDX, IDC_CH_AUTOFOCUS, m_chAutoFocus);
	DDX_Control(pDX, IDC_SL_FOCUS, m_slFocus);
	DDX_Control(pDX, IDC_ST_FOCUS_VAL, m_stFocusValue);
	DDX_Control(pDX, IDC_SL_BRIGHTNESS, m_slBrightness);
	DDX_Control(pDX, IDC_ST_BRIGHTNESS_VAL, m_stBrightnessValue);
	DDX_Control(pDX, IDC_SL_CONTRAST, m_slContrast);
	DDX_Control(pDX, IDC_ST_CONTRAST_VAL, m_stContrastValue);
	DDX_Control(pDX, IDC_SL_SATURATION, m_slSaturation);
	DDX_Control(pDX, IDC_ST_SATURATION_VAL, m_stSaturationValue);

	if (pDX->m_bSaveAndValidate)
		m_iMotorIntervalTimer = min(max(10,m_iMotorIntervalTimer),1000);
}


BEGIN_MESSAGE_MAP(CSettingsDlg, CDialogEx)
	ON_BN_CLICKED(IDC_CH_LOGITECHCONTROL, &CSettingsDlg::OnChLogitechcontrol)
	ON_WM_HSCROLL()
	ON_BN_CLICKED(IDC_CH_AUTOFOCUS, &CSettingsDlg::OnChAutoFocus)
	ON_BN_CLICKED(IDC_BT_RESET_IMAGE, &CSettingsDlg::OnBtResetImage)
END_MESSAGE_MAP()


// CSettingsDlg message handlers


void CSettingsDlg::OnChLogitechcontrol()
{
	m_edMotorInterval.EnableWindow(!m_chLogitechControl.GetCheck());
}

void CSettingsDlg::OnChAutoFocus()
{
	BOOL bAuto = m_chAutoFocus.GetCheck();
	m_slFocus.EnableWindow(!bAuto);
	theApp.WriteProfileInt(REG_DEVICE, _T("AutoFocus"), bAuto);
	if (bAuto)
	{
		if (m_pCurrentWebCam)
			m_pCurrentWebCam->SetFocus(0, CameraControl_Flags_Auto);
		m_stFocusValue.SetWindowText(_T("Auto"));
	}
	else
	{
		long lPos = m_slFocus.GetPos();
		if (m_pCurrentWebCam)
			m_pCurrentWebCam->SetFocus(lPos, CameraControl_Flags_Manual);
		theApp.WriteProfileInt(REG_DEVICE, _T("FocusManual"), lPos);
		CString str;
		str.Format(_T("%ld"), lPos);
		m_stFocusValue.SetWindowText(str);
	}
}

void CSettingsDlg::OnHScroll(UINT nSBCode, UINT nPos, CScrollBar* pScrollBar)
{
	if (!pScrollBar)
	{
		CDialogEx::OnHScroll(nSBCode, nPos, pScrollBar);
		return;
	}

	HWND hScrollBar = pScrollBar->GetSafeHwnd();

	if (hScrollBar == m_slFocus.GetSafeHwnd())
	{
		long val = m_slFocus.GetPos();
		m_chAutoFocus.SetCheck(BST_UNCHECKED);
		if (m_pCurrentWebCam)
			m_pCurrentWebCam->SetFocus(val, CameraControl_Flags_Manual);
		theApp.WriteProfileInt(REG_DEVICE, _T("FocusManual"), val);
		theApp.WriteProfileInt(REG_DEVICE, _T("AutoFocus"), 0);
		CString str;
		str.Format(_T("%ld"), val);
		m_stFocusValue.SetWindowText(str);
	}
	else if (hScrollBar == m_slBrightness.GetSafeHwnd())
	{
		long val = m_slBrightness.GetPos();
		if (m_pCurrentWebCam)
			m_pCurrentWebCam->SetBrightness(val, VideoProcAmp_Flags_Manual);
		theApp.WriteProfileInt(REG_DEVICE, _T("Brightness"), val);
		CString str;
		str.Format(_T("%ld"), val);
		m_stBrightnessValue.SetWindowText(str);
	}
	else if (hScrollBar == m_slContrast.GetSafeHwnd())
	{
		long val = m_slContrast.GetPos();
		if (m_pCurrentWebCam)
			m_pCurrentWebCam->SetContrast(val, VideoProcAmp_Flags_Manual);
		theApp.WriteProfileInt(REG_DEVICE, _T("Contrast"), val);
		CString str;
		str.Format(_T("%ld"), val);
		m_stContrastValue.SetWindowText(str);
	}
	else if (hScrollBar == m_slSaturation.GetSafeHwnd())
	{
		long val = m_slSaturation.GetPos();
		if (m_pCurrentWebCam)
			m_pCurrentWebCam->SetSaturation(val, VideoProcAmp_Flags_Manual);
		theApp.WriteProfileInt(REG_DEVICE, _T("Saturation"), val);
		CString str;
		str.Format(_T("%ld"), val);
		m_stSaturationValue.SetWindowText(str);
	}

	CDialogEx::OnHScroll(nSBCode, nPos, pScrollBar);
}

void CSettingsDlg::OnBtResetImage()
{
	long lMin = 0, lMax = 0, lStep = 0, lDef = 0, lFlags = 0;

	// Reset Focus
	m_chAutoFocus.SetCheck(BST_CHECKED);
	m_slFocus.EnableWindow(FALSE);
	m_stFocusValue.SetWindowText(_T("Auto"));
	theApp.WriteProfileInt(REG_DEVICE, _T("AutoFocus"), 1);
	if (m_pCurrentWebCam && SUCCEEDED(m_pCurrentWebCam->GetFocusRange(lMin, lMax, lStep, lDef, lFlags)))
	{
		m_pCurrentWebCam->SetFocus(lDef, CameraControl_Flags_Auto);
		m_slFocus.SetPos(lDef);
	}
	else
	{
		m_slFocus.SetPos(50);
	}

	// Reset Brightness
	long defBrightness = 0;
	if (m_pCurrentWebCam && SUCCEEDED(m_pCurrentWebCam->GetBrightnessRange(lMin, lMax, lStep, lDef, lFlags)))
	{
		defBrightness = lDef;
		m_pCurrentWebCam->SetBrightness(lDef, VideoProcAmp_Flags_Manual);
	}
	m_slBrightness.SetPos(defBrightness);
	theApp.WriteProfileInt(REG_DEVICE, _T("Brightness"), defBrightness);
	CString str; str.Format(_T("%ld"), defBrightness);
	m_stBrightnessValue.SetWindowText(str);

	// Reset Contrast
	long defContrast = 50;
	if (m_pCurrentWebCam && SUCCEEDED(m_pCurrentWebCam->GetContrastRange(lMin, lMax, lStep, lDef, lFlags)))
	{
		defContrast = lDef;
		m_pCurrentWebCam->SetContrast(lDef, VideoProcAmp_Flags_Manual);
	}
	m_slContrast.SetPos(defContrast);
	theApp.WriteProfileInt(REG_DEVICE, _T("Contrast"), defContrast);
	str.Format(_T("%ld"), defContrast);
	m_stContrastValue.SetWindowText(str);

	// Reset Saturation
	long defSaturation = 50;
	if (m_pCurrentWebCam && SUCCEEDED(m_pCurrentWebCam->GetSaturationRange(lMin, lMax, lStep, lDef, lFlags)))
	{
		defSaturation = lDef;
		m_pCurrentWebCam->SetSaturation(lDef, VideoProcAmp_Flags_Manual);
	}
	m_slSaturation.SetPos(defSaturation);
	theApp.WriteProfileInt(REG_DEVICE, _T("Saturation"), defSaturation);
	str.Format(_T("%ld"), defSaturation);
	m_stSaturationValue.SetWindowText(str);
}


BOOL CSettingsDlg::OnInitDialog()
{
	CDialogEx::OnInitDialog();

	CFileVersionInfo fvi;
	fvi.GetFileVersionInfo();
	CString strAppVersion;
	strAppVersion.Format(_T("%d.%d.%d"), HIWORD(fvi.dwFileVersionMS),LOWORD(fvi.dwFileVersionMS),HIWORD(fvi.dwFileVersionLS));

	CString strText;
	GetWindowText(strText);
	strText += strAppVersion;
	SetWindowText(strText);

	OnChLogitechcontrol();

	// Initialize Image & Optical sliders
	long lMin = 0, lMax = 0, lStep = 0, lDef = 0, lFlags = 0, lVal = 0;

	// Focus
	if (m_pCurrentWebCam && SUCCEEDED(m_pCurrentWebCam->GetFocusRange(lMin, lMax, lStep, lDef, lFlags)))
	{
		m_slFocus.SetRange(lMin, lMax);
		m_slFocus.SetLineSize(lStep > 0 ? lStep : 1);
		if (SUCCEEDED(m_pCurrentWebCam->GetFocus(lVal, lFlags)))
		{
			m_slFocus.SetPos(lVal);
			bool bAuto = (lFlags & CameraControl_Flags_Auto) != 0;
			m_chAutoFocus.SetCheck(bAuto ? BST_CHECKED : BST_UNCHECKED);
			m_slFocus.EnableWindow(bAuto ? FALSE : TRUE);
			CString str = bAuto ? _T("Auto") : _T("");
			if (!bAuto) str.Format(_T("%ld"), lVal);
			m_stFocusValue.SetWindowText(str);
		}
	}
	else
	{
		m_slFocus.SetRange(0, 100);
		m_slFocus.SetLineSize(5);
		int savedFocus = theApp.GetProfileInt(REG_DEVICE, _T("FocusManual"), 50);
		int savedAF = theApp.GetProfileInt(REG_DEVICE, _T("AutoFocus"), 1);
		m_slFocus.SetPos(savedFocus);
		m_chAutoFocus.SetCheck(savedAF ? BST_CHECKED : BST_UNCHECKED);
		m_slFocus.EnableWindow(!savedAF);
		m_stFocusValue.SetWindowText(savedAF ? _T("Auto") : _T("50"));
	}

	// Brightness
	if (m_pCurrentWebCam && SUCCEEDED(m_pCurrentWebCam->GetBrightnessRange(lMin, lMax, lStep, lDef, lFlags)))
	{
		m_slBrightness.SetRange(lMin, lMax);
		m_slBrightness.SetLineSize(lStep > 0 ? lStep : 1);
		if (SUCCEEDED(m_pCurrentWebCam->GetBrightness(lVal, lFlags)))
		{
			m_slBrightness.SetPos(lVal);
			CString str; str.Format(_T("%ld"), lVal);
			m_stBrightnessValue.SetWindowText(str);
		}
	}
	else
	{
		m_slBrightness.SetRange(-64, 64);
		m_slBrightness.SetLineSize(1);
		int savedBright = theApp.GetProfileInt(REG_DEVICE, _T("Brightness"), 0);
		m_slBrightness.SetPos(savedBright);
		CString str; str.Format(_T("%d"), savedBright);
		m_stBrightnessValue.SetWindowText(str);
	}

	// Contrast
	if (m_pCurrentWebCam && SUCCEEDED(m_pCurrentWebCam->GetContrastRange(lMin, lMax, lStep, lDef, lFlags)))
	{
		m_slContrast.SetRange(lMin, lMax);
		m_slContrast.SetLineSize(lStep > 0 ? lStep : 1);
		if (SUCCEEDED(m_pCurrentWebCam->GetContrast(lVal, lFlags)))
		{
			m_slContrast.SetPos(lVal);
			CString str; str.Format(_T("%ld"), lVal);
			m_stContrastValue.SetWindowText(str);
		}
	}
	else
	{
		m_slContrast.SetRange(0, 100);
		m_slContrast.SetLineSize(1);
		int savedContrast = theApp.GetProfileInt(REG_DEVICE, _T("Contrast"), 50);
		m_slContrast.SetPos(savedContrast);
		CString str; str.Format(_T("%d"), savedContrast);
		m_stContrastValue.SetWindowText(str);
	}

	// Saturation
	if (m_pCurrentWebCam && SUCCEEDED(m_pCurrentWebCam->GetSaturationRange(lMin, lMax, lStep, lDef, lFlags)))
	{
		m_slSaturation.SetRange(lMin, lMax);
		m_slSaturation.SetLineSize(lStep > 0 ? lStep : 1);
		if (SUCCEEDED(m_pCurrentWebCam->GetSaturation(lVal, lFlags)))
		{
			m_slSaturation.SetPos(lVal);
			CString str; str.Format(_T("%ld"), lVal);
			m_stSaturationValue.SetWindowText(str);
		}
	}
	else
	{
		m_slSaturation.SetRange(0, 100);
		m_slSaturation.SetLineSize(1);
		int savedSat = theApp.GetProfileInt(REG_DEVICE, _T("Saturation"), 50);
		m_slSaturation.SetPos(savedSat);
		CString str; str.Format(_T("%d"), savedSat);
		m_stSaturationValue.SetWindowText(str);
	}

	CenterWindow();
	return TRUE;  				  
}
