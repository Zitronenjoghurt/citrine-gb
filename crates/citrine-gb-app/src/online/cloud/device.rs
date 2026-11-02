#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn device_name() -> String {
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
pub(crate) fn device_name() -> String {
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
