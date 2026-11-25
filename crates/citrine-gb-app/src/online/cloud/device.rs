pub(crate) fn device_name() -> String {
    named("", system_name)
}

pub(crate) fn device_name_from(chosen: &str) -> String {
    named(chosen, system_name)
}

fn named(chosen: &str, system: impl FnOnce() -> String) -> String {
    if chosen.trim().is_empty() {
        citrine_gb_net::device_name(&system())
    } else {
        citrine_gb_net::device_name(chosen)
    }
}

#[cfg(target_os = "android")]
pub fn system_name() -> String {
    crate::platform::android::device_name().unwrap_or_else(os_name)
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub fn system_name() -> String {
    static NAME: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    NAME.get_or_init(|| {
        computer_name()
            .map(|name| name.trim().to_owned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(os_name)
    })
    .clone()
}

#[cfg(target_os = "macos")]
fn computer_name() -> Option<String> {
    let out = std::process::Command::new("scutil")
        .args(["--get", "ComputerName"])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(windows)]
fn computer_name() -> Option<String> {
    std::env::var("COMPUTERNAME").ok()
}

#[cfg(not(any(
    target_arch = "wasm32",
    target_os = "android",
    target_os = "macos",
    windows
)))]
fn computer_name() -> Option<String> {
    let pretty = std::fs::read_to_string("/etc/machine-info")
        .ok()
        .and_then(|info| pretty_hostname(&info));
    pretty.or_else(|| std::fs::read_to_string("/etc/hostname").ok())
}

#[cfg_attr(
    any(
        target_arch = "wasm32",
        target_os = "android",
        target_os = "macos",
        windows
    ),
    allow(dead_code)
)]
fn pretty_hostname(info: &str) -> Option<String> {
    let value = info
        .lines()
        .find_map(|line| line.trim().strip_prefix("PRETTY_HOSTNAME="))?;
    Some(value.trim().trim_matches('"').to_owned())
}

#[cfg(not(target_arch = "wasm32"))]
fn os_name() -> String {
    match std::env::consts::OS {
        "macos" => "Mac",
        "windows" => "Windows PC",
        "linux" => "Linux PC",
        "android" => "Android phone",
        "ios" => "iPhone",
        _ => "Computer",
    }
    .to_string()
}

#[cfg(target_arch = "wasm32")]
pub fn system_name() -> String {
    let agent = web_sys::window()
        .and_then(|w| w.navigator().user_agent().ok())
        .unwrap_or_default();
    browser_on(&agent)
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(super) fn browser_on(agent: &str) -> String {
    let browser = if agent.contains("Firefox/") {
        "Firefox"
    } else if agent.contains("Edg/") {
        "Edge"
    } else if agent.contains("OPR/") {
        "Opera"
    } else if agent.contains("Chrome/") {
        "Chrome"
    } else if agent.contains("Safari/") {
        "Safari"
    } else {
        "Browser"
    };
    let system = if agent.contains("Android") {
        "Android"
    } else if agent.contains("iPhone") {
        "iPhone"
    } else if agent.contains("iPad") {
        "iPad"
    } else if agent.contains("Mac OS X") {
        "Mac"
    } else if agent.contains("Windows") {
        "Windows"
    } else if agent.contains("Linux") || agent.contains("CrOS") {
        "Linux"
    } else {
        return browser.to_string();
    };
    format!("{browser} on {system}")
}

#[cfg(test)]
mod tests {
    use super::{browser_on, named};

    #[test]
    fn the_pretty_hostname_comes_from_machine_info() {
        let info = "DEPLOYMENT=x\nPRETTY_HOSTNAME=\"Clara's Desk\"\n";
        assert_eq!(
            super::pretty_hostname(info).as_deref(),
            Some("Clara's Desk")
        );
        assert_eq!(super::pretty_hostname("CHASSIS=desktop"), None);
    }

    #[test]
    fn a_chosen_name_wins_and_an_empty_one_falls_back_to_the_system() {
        let system = || "Clara's MacBook Pro".to_owned();
        assert_eq!(named("", system), "Clara's MacBook Pro");
        assert_eq!(named("  ", system), "Clara's MacBook Pro");
        assert_eq!(named(" Desk Mac ", system), "Desk Mac");
    }

    #[test]
    fn devices_read_as_browser_and_system() {
        let firefox = "Mozilla/5.0 (Android 14; Mobile; rv:131.0) Gecko/131.0 Firefox/131.0";
        assert_eq!(browser_on(firefox), "Firefox on Android");
        let chrome = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Safari/537.36";
        assert_eq!(browser_on(chrome), "Chrome on Mac");
    }
}
