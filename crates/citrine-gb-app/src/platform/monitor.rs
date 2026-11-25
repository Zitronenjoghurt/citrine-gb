use crate::app::monitor::{Monitor, Rect, Screens};
#[cfg(any(windows, target_os = "macos"))]
use raw_window_handle::HasWindowHandle;

#[cfg(windows)]
pub use windows::OsScreens;

#[cfg(target_os = "macos")]
pub use mac::OsScreens;

#[cfg(not(any(windows, target_os = "macos")))]
pub struct OsScreens;

#[cfg(not(any(windows, target_os = "macos")))]
impl OsScreens {
    pub fn of(_frame: &eframe::Frame) -> Option<Self> {
        Some(Self)
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
impl Screens for OsScreens {
    fn window(&mut self) -> Option<Rect> {
        None
    }

    fn monitors(&mut self) -> Vec<Monitor> {
        Vec::new()
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use crate::app::monitor::{MonitorRate, RateSource};
    use raw_window_handle::RawWindowHandle;
    use windows_sys::Win32::Devices::Display::{
        DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_MODE_INFO,
        DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_SOURCE_DEVICE_NAME, DisplayConfigGetDeviceInfo,
        GetDisplayConfigBufferSizes, QDC_ONLY_ACTIVE_PATHS, QueryDisplayConfig,
    };
    use windows_sys::Win32::Foundation::{ERROR_SUCCESS, HWND, LPARAM, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        DEVMODEW, ENUM_CURRENT_SETTINGS, EnumDisplayMonitors, EnumDisplaySettingsW,
        GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect;
    use windows_sys::core::BOOL;

    pub struct OsScreens {
        hwnd: HWND,
    }

    impl OsScreens {
        pub fn of(window: &impl HasWindowHandle) -> Option<Self> {
            match window.window_handle().ok()?.as_raw() {
                RawWindowHandle::Win32(handle) => Some(Self {
                    hwnd: handle.hwnd.get() as HWND,
                }),
                _ => None,
            }
        }
    }

    impl Screens for OsScreens {
        fn window(&mut self) -> Option<Rect> {
            let mut rect = RECT::default();
            (unsafe { GetWindowRect(self.hwnd, &mut rect) } != 0).then(|| rect_of(&rect))
        }

        fn monitors(&mut self) -> Vec<Monitor> {
            let mut handles: Vec<HMONITOR> = Vec::new();
            unsafe {
                EnumDisplayMonitors(
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    Some(push_monitor),
                    &mut handles as *mut Vec<HMONITOR> as LPARAM,
                );
            }
            let rates = display_config_rates();
            handles
                .into_iter()
                .filter_map(|handle| {
                    let mut info = MONITORINFOEXW::default();
                    info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
                    let read = unsafe {
                        GetMonitorInfoW(
                            handle,
                            &mut info as *mut MONITORINFOEXW as *mut MONITORINFO,
                        )
                    };
                    if read == 0 {
                        return None;
                    }
                    let device = info.szDevice;
                    let rate = rates
                        .iter()
                        .find(|(name, _)| same_name(name, &device))
                        .map(|&(_, hz)| MonitorRate {
                            hz,
                            source: RateSource::WindowsDisplayConfig,
                        })
                        .or_else(|| display_settings_rate(&device));
                    Some(Monitor {
                        id: handle as usize as u64,
                        rect: rect_of(&info.monitorInfo.rcMonitor),
                        rate,
                    })
                })
                .collect()
        }
    }

    unsafe extern "system" fn push_monitor(
        handle: HMONITOR,
        _dc: HDC,
        _rect: *mut RECT,
        handles: LPARAM,
    ) -> BOOL {
        let handles = unsafe { &mut *(handles as *mut Vec<HMONITOR>) };
        handles.push(handle);
        1
    }

    fn rect_of(rect: &RECT) -> Rect {
        Rect {
            min: [f64::from(rect.left), f64::from(rect.top)],
            max: [f64::from(rect.right), f64::from(rect.bottom)],
        }
    }

    fn same_name(a: &[u16; 32], b: &[u16; 32]) -> bool {
        let end = |name: &[u16; 32]| name.iter().position(|&c| c == 0).unwrap_or(name.len());
        a[..end(a)] == b[..end(b)]
    }

    fn display_config_rates() -> Vec<([u16; 32], f64)> {
        let mut paths = 0;
        let mut modes = 0;
        if unsafe { GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut paths, &mut modes) }
            != ERROR_SUCCESS
        {
            return Vec::new();
        }
        let mut path_list = vec![DISPLAYCONFIG_PATH_INFO::default(); paths as usize];
        let mut mode_list = vec![DISPLAYCONFIG_MODE_INFO::default(); modes as usize];
        let queried = unsafe {
            QueryDisplayConfig(
                QDC_ONLY_ACTIVE_PATHS,
                &mut paths,
                path_list.as_mut_ptr(),
                &mut modes,
                mode_list.as_mut_ptr(),
                std::ptr::null_mut(),
            )
        };
        if queried != ERROR_SUCCESS {
            return Vec::new();
        }
        path_list.truncate(paths as usize);
        path_list
            .iter()
            .filter_map(|path| {
                let rate = path.targetInfo.refreshRate;
                if rate.Denominator == 0 || rate.Numerator == 0 {
                    return None;
                }
                let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME::default();
                source.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
                source.header.size = size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32;
                source.header.adapterId = path.sourceInfo.adapterId;
                source.header.id = path.sourceInfo.id;
                let named = unsafe { DisplayConfigGetDeviceInfo(&mut source.header) };
                (named == ERROR_SUCCESS as i32).then(|| {
                    (
                        source.viewGdiDeviceName,
                        f64::from(rate.Numerator) / f64::from(rate.Denominator),
                    )
                })
            })
            .collect()
    }

    fn display_settings_rate(device: &[u16; 32]) -> Option<MonitorRate> {
        let mut mode = DEVMODEW {
            dmSize: size_of::<DEVMODEW>() as u16,
            ..Default::default()
        };
        let read =
            unsafe { EnumDisplaySettingsW(device.as_ptr(), ENUM_CURRENT_SETTINGS, &mut mode) };
        (read != 0 && mode.dmDisplayFrequency > 1).then(|| MonitorRate {
            hz: f64::from(mode.dmDisplayFrequency),
            source: RateSource::WindowsDisplaySettings,
        })
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use super::*;
    use crate::app::monitor::{MonitorRate, RateSource};
    use objc2::encode::{Encode, Encoding};
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send, sel};
    use raw_window_handle::RawWindowHandle;
    use std::ffi::c_void;

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGDisplayCopyDisplayMode(display: u32) -> *mut c_void;
        fn CGDisplayModeGetRefreshRate(mode: *mut c_void) -> f64;
        fn CGDisplayModeRelease(mode: *mut c_void);
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Pair {
        a: f64,
        b: f64,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Frame {
        origin: Pair,
        size: Pair,
    }

    const POINT: Encoding = Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]);
    const SIZE: Encoding = Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]);

    unsafe impl Encode for Frame {
        const ENCODING: Encoding = Encoding::Struct("CGRect", &[POINT, SIZE]);
    }

    pub struct OsScreens {
        view: *mut AnyObject,
    }

    impl OsScreens {
        pub fn of(window: &impl HasWindowHandle) -> Option<Self> {
            match window.window_handle().ok()?.as_raw() {
                RawWindowHandle::AppKit(handle) => Some(Self {
                    view: handle.ns_view.as_ptr().cast(),
                }),
                _ => None,
            }
        }
    }

    impl Screens for OsScreens {
        fn window(&mut self) -> Option<Rect> {
            let window: *mut AnyObject = unsafe { msg_send![self.view, window] };
            if window.is_null() {
                return None;
            }
            let frame: Frame = unsafe { msg_send![window, frame] };
            Some(rect_of(frame))
        }

        fn monitors(&mut self) -> Vec<Monitor> {
            let screens: *mut AnyObject = unsafe { msg_send![class!(NSScreen), screens] };
            if screens.is_null() {
                return Vec::new();
            }
            let count: usize = unsafe { msg_send![screens, count] };
            (0..count)
                .filter_map(|index| {
                    let screen: *mut AnyObject =
                        unsafe { msg_send![screens, objectAtIndex: index] };
                    let id = display_id(screen)?;
                    let frame: Frame = unsafe { msg_send![screen, frame] };
                    Some(Monitor {
                        id: u64::from(id),
                        rect: rect_of(frame),
                        rate: mode_rate(id).or_else(|| screen_maximum(screen)),
                    })
                })
                .collect()
        }
    }

    fn rect_of(frame: Frame) -> Rect {
        Rect {
            min: [frame.origin.a, frame.origin.b],
            max: [frame.origin.a + frame.size.a, frame.origin.b + frame.size.b],
        }
    }

    fn display_id(screen: *mut AnyObject) -> Option<u32> {
        if screen.is_null() {
            return None;
        }
        let description: *mut AnyObject = unsafe { msg_send![screen, deviceDescription] };
        if description.is_null() {
            return None;
        }
        let key = objc2_foundation::ns_string!("NSScreenNumber");
        let number: *mut AnyObject = unsafe { msg_send![description, objectForKey: key] };
        if number.is_null() {
            return None;
        }
        Some(unsafe { msg_send![number, unsignedIntValue] })
    }

    fn mode_rate(id: u32) -> Option<MonitorRate> {
        let mode = unsafe { CGDisplayCopyDisplayMode(id) };
        if mode.is_null() {
            return None;
        }
        let hz = unsafe { CGDisplayModeGetRefreshRate(mode) };
        unsafe { CGDisplayModeRelease(mode) };
        (hz > 1.0).then_some(MonitorRate {
            hz,
            source: RateSource::MacDisplayMode,
        })
    }

    fn screen_maximum(screen: *mut AnyObject) -> Option<MonitorRate> {
        let known: bool =
            unsafe { msg_send![screen, respondsToSelector: sel!(maximumFramesPerSecond)] };
        if !known {
            return None;
        }
        let fps: isize = unsafe { msg_send![screen, maximumFramesPerSecond] };
        (fps > 1).then_some(MonitorRate {
            hz: fps as f64,
            source: RateSource::MacScreenMaximum,
        })
    }
}
