use super::{GroupId, SettingsTab, Surface};
use Field::{Action, Many, One, Pending, PerSurface, Server};
use strum_macros::EnumIter;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    One(&'static str),
    Many(&'static [&'static str]),
    PerSurface {
        desktop: &'static [&'static str],
        phone: &'static [&'static str],
    },
    Action,
    Server,
    Pending(&'static str),
}

impl Field {
    pub fn paths(self, surface: Surface) -> Vec<&'static str> {
        match (self, surface) {
            (One(path), _) => vec![path],
            (Many(paths), _) => paths.to_vec(),
            (PerSurface { desktop, .. }, Surface::Desktop) => desktop.to_vec(),
            (PerSurface { phone, .. }, Surface::Phone) => phone.to_vec(),
            (Action | Server | Pending(_), _) => Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum On {
    Both,
    Desktop,
    Phone,
}

impl On {
    fn surfaces(self) -> &'static [Surface] {
        match self {
            Self::Both => &[Surface::Desktop, Surface::Phone],
            Self::Desktop => &[Surface::Desktop],
            Self::Phone => &[Surface::Phone],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Need {
    Nds,
    Jit,
    DsWifi,
    NotWeb,
    Web,
    Android,
    NotAndroid,
    NativeDesktop,
    CiscoCodec,
}

impl Need {
    fn met(self) -> bool {
        let web = cfg!(target_arch = "wasm32");
        let android = cfg!(target_os = "android");
        match self {
            Self::Nds => cfg!(feature = "nds"),
            Self::Jit => jit_available(),
            Self::DsWifi => cfg!(all(feature = "nds", ds_wifi)),
            Self::NotWeb => !web,
            Self::Web => web,
            Self::Android => android,
            Self::NotAndroid => !android,
            Self::NativeDesktop => !web && !android,
            Self::CiscoCodec => !web && !cfg!(feature = "openh264-source"),
        }
    }
}

#[cfg(feature = "nds")]
fn jit_available() -> bool {
    citrine_gb::nds::machine::JIT_AVAILABLE
}

#[cfg(not(feature = "nds"))]
fn jit_available() -> bool {
    false
}

struct Spec {
    group: GroupId,
    label: &'static str,
    phone_label: Option<&'static str>,
    on: On,
    needs: &'static [Need],
    field: Field,
    keywords: &'static [&'static str],
    per_game: bool,
}

macro_rules! rows {
    (@phone) => { None };
    (@phone $phone:literal) => { Some($phone) };
    (@scope) => { false };
    (@scope PerGame) => { true };
    ($($id:ident: $group:ident, $label:literal $(| $phone:literal)?, $on:ident, [$($need:ident),*], $field:expr, [$($keyword:literal),*] $(, $scope:ident)?;)*) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, EnumIter)]
        pub enum RowId {
            $($id,)*
        }

        const SPECS: &[Spec] = &[
            $(Spec {
                group: GroupId::$group,
                label: $label,
                phone_label: rows!(@phone $($phone)?),
                on: On::$on,
                needs: &[$(Need::$need),*],
                field: $field,
                keywords: &[$($keyword),*],
                per_game: rows!(@scope $($scope)?),
            },)*
        ];
    };
}

rows! {
    Scaling: Size, "Scaling", Both, [], PerSurface { desktop: &["scaling"], phone: &["touch.scaling"] }, ["zoom", "integer", "fit", "stretch"], PerGame;
    SameSizePixels: Size, "Every pixel is the same size", Both, [], One("same_size_pixels"), ["same size", "integer", "square pixels", "shimmer"], PerGame;
    MatchRefresh: Size, "Match refresh rate", Both, [], One("match_screen"), ["60 hz", "vsync", "stutter", "frame rate"];
    LcdGrid: Effects, "LCD grid", Both, [], One("matrix"), ["pixel grid", "lines between pixels"], PerGame;
    GridLines: Effects, "Grid lines", Both, [], One("matrix_edge_darkness"), ["line darkness"], PerGame;
    GridCorners: Effects, "Grid corners", Both, [], One("matrix_corner_darkness"), ["crossing darkness"], PerGame;
    Ghosting: Effects, "Ghosting", Both, [], One("ghosting"), ["motion blur", "smear"], PerGame;
    GhostingStrength: Effects, "Ghosting strength", Both, [], One("ghosting_strength"), ["blur amount"], PerGame;
    ShowHuds: Huds, "Show HUDs", Both, [], One("hud.shown"), ["overlay", "fps", "stats"];
    HudOpacity: Huds, "HUD opacity", Both, [], One("hud.opacity"), ["hud card", "transparency"];

    Volume: Output, "Volume", Both, [], One("volume"), ["loudness"], PerGame;
    Mute: Output, "Mute", Both, [], One("muted"), ["silent", "sound off"];
    OutputDevice: Output, "Output device", Both, [NotWeb], One("audio_device"), ["speakers", "headphones", "audio device"];
    Latency: Timing, "Latency", Both, [], One("audio_queue_ms"), ["audio delay", "crackle", "buffer"];
    SlowSound: Timing, "If the game runs slow", Both, [], One("slow_sound"), ["slowdown", "tempo", "pitch", "when the game runs slow"], PerGame;
    FastForwardSound: Timing, "While fast forwarding", Both, [], One("fast_forward_sound"), ["fast forward sound", "turbo sound"], PerGame;

    GameButtons: Keys, "Game buttons", Both, [], Many(&["bindings.keyboard", "bindings.gamepad", "bindings.shoulders", "bindings.ds", "bindings.combos"]), ["keys", "bindings", "remap", "keyboard", "gamepad", "combination", "combo"];
    Shortcuts: Keys, "Shortcuts", Both, [], One("hotkeys"), ["hotkeys", "keyboard shortcuts"];
    StickSteers: Controller, "Left stick steers", Both, [], One("bindings.stick_as_dpad"), ["analog stick", "d-pad", "joystick"];
    StickDeadZone: Controller, "Stick dead zone", Both, [], One("bindings.stick_deadzone"), ["deadzone", "drift"];
    Rumble: Controller, "Rumble", Both, [], One("gimmicks.rumble"), ["vibration", "force feedback"];
    TiltKeys: Controller, "Tilt keys", Both, [], One("bindings.tilt"), ["tilt", "gyro", "motion", "accelerometer"];
    ControllerTest: Controller, "Controller test", Both, [], Action, ["controller debug", "gamepad test"];
    SplitKeys: Split, "Keys for each half", Both, [], Many(&["local_link.left_keys", "local_link.right_keys"]), ["split keyboard", "left half", "right half", "two players"];
    TouchShow: Touch, "Show", Both, [], One("touch.show_controls"), ["on-screen buttons", "touch controls", "virtual gamepad"];
    TouchSize: Touch, "Size", Both, [], One("touch.scale"), ["button size"];
    TouchOpacity: Touch, "Opacity", Both, [], One("touch.opacity"), ["button opacity", "transparency"];
    TouchScaling: Touch, "Scaling with buttons", Desktop, [], One("touch.scaling"), ["around buttons", "game scaling", "screen scaling", "zoom"];
    TouchLandscape: Touch, "In landscape", Both, [], One("touch.landscape"), ["beside the game", "landscape buttons"];
    TouchHidden: Touch, "With menus hidden", Both, [], One("touch.hidden_buttons"), ["hidden ui", "hide buttons"];
    LeftHanded: Touch, "Left-handed", Both, [], One("touch.mirrored"), ["mirror", "d-pad on the right"];
    Haptics: Touch, "Vibrate on press", Both, [], One("touch.haptics"), ["haptics", "vibration"];
    BigScreenMode: BigScreen, "Big screen mode", Both, [], One("big_screen.on"), ["tv", "couch", "controller menus"];
    MenuButton: BigScreen, "Menu button", Phone, [], Action, ["home button", "select and start"];
    ConfirmButton: BigScreen, "Confirm button", Both, [], One("big_screen.confirm"), ["accept button", "a or b"];
    ButtonIcons: BigScreen, "Button icons", Both, [], One("big_screen.icons"), ["xbox", "playstation", "nintendo", "glyphs"];
    AroundTheGame: BigScreen, "Around the game", Both, [], One("big_screen.backdrop"), ["backdrop", "border"];

    FastForward: Speed, "Fast forward", Both, [], One("fast_forward_speed"), ["turbo", "speed up"], PerGame;
    FastForwardKey: Speed, "Fast forward key", Both, [], One("fast_forward_key"), ["hold or toggle", "turbo key"];
    Rewind: Speed, "Rewind", Both, [], One("rewind"), ["how far back", "go back"], PerGame;
    AutosaveEvery: Saves, "Autosave every", Both, [], One("autosave_interval"), ["autosave interval", "save state"];
    AutosavesKept: Saves, "Autosaves kept", Both, [], One("autosave_keep"), ["autosave count"];
    QuickLoad: Saves, "Quick load", Both, [], One("quick_load"), ["load state", "hold to load"];
    InBackground: Pause, "In the background", Both, [], One("background_pause"), ["pause", "out of focus", "focus lost", "minimised", "mute in background"];

    ScreenshotSize: Capture, "Screenshot size", Both, [], One("screenshot_scale"), ["screenshot scale", "picture size"];
    RecordingSize: Capture, "Recording size", Both, [], One("recording_scale"), ["video size", "recording scale"];
    RenderVideo: Capture, "Render the video", Both, [], One("render_video"), ["render", "make the video", "when to render"];
    ClipLength: Capture, "Clip length", Both, [], One("clip_length"), ["save a clip", "how far back", "instant replay"];
    CaptureFolder: Capture, "Folder", Desktop, [NativeDesktop], One("capture_folder"), ["screenshot folder", "recording folder", "save location"];
    VideoCodec: Capture, "Video codec", Desktop, [CiscoCodec], Action, ["openh264", "h264", "download codec"];

    RunGamesOn: GbSystem, "Run games on", Both, [], One("console"), ["console", "model", "dmg", "cgb", "game boy color"], PerGame;
    GbBootRom: GbSystem, "GB boot ROM", Both, [], Action, ["boot rom", "game boy boot rom", "bios", "dmg", "startup logo"];
    GbcBootRom: GbSystem, "GBC boot ROM", Both, [], Action, ["boot rom", "bios", "cgb", "startup logo", "game boy color boot rom", "color boot rom"];
    CameraSees: GbSystem, "Game Boy Camera sees", Both, [], One("gimmicks.camera"), ["camera", "webcam"];
    ChoosePicture: GbSystem, "Camera picture", Both, [], Action, ["choose picture", "photo"];
    Palette: GbColours, "Palette", Both, [], Many(&["dmg_theme", "custom_palette", "gbc_palette"]), ["colours", "colors", "green", "theme"], PerGame;
    OwnPalettes: GbColours, "Own palettes", Both, [], One("palettes.entries"), ["custom palette", "share code", "palette editor"];
    GbcScreen: GbColours, "Screen colours", Both, [], One("color_correction"), ["colour correction", "color correction", "gbc screen", "game boy color screen", "gbc colours"], PerGame;
    GbPulse1: GbChannels, "Pulse 1", Both, [], One("channels"), ["sound channel", "mute channel"], PerGame;
    GbPulse2: GbChannels, "Pulse 2", Both, [], One("channels"), ["sound channel", "mute channel"], PerGame;
    GbWave: GbChannels, "Wave", Both, [], One("channels"), ["sound channel", "mute channel"], PerGame;
    GbNoise: GbChannels, "Noise", Both, [], One("channels"), ["sound channel", "mute channel"], PerGame;

    GbaBios: GbaSystem, "BIOS", Both, [], Action, ["gba bios", "boot"];
    GbaScreen: GbaColours, "Screen colours", Both, [], One("gba_color_correction"), ["colour correction", "color correction", "gba screen"], PerGame;
    GbaPulse1: GbaChannels, "Pulse 1", Both, [], One("gba_channels"), ["sound channel", "mute channel"], PerGame;
    GbaPulse2: GbaChannels, "Pulse 2", Both, [], One("gba_channels"), ["sound channel", "mute channel"], PerGame;
    GbaWave: GbaChannels, "Wave", Both, [], One("gba_channels"), ["sound channel", "mute channel"], PerGame;
    GbaNoise: GbaChannels, "Noise", Both, [], One("gba_channels"), ["sound channel", "mute channel"], PerGame;
    DirectSoundA: GbaChannels, "Direct sound A", Both, [], One("gba_channels"), ["sound channel", "mute channel"], PerGame;
    DirectSoundB: GbaChannels, "Direct sound B", Both, [], One("gba_channels"), ["sound channel", "mute channel"], PerGame;
    GbaJit: GbaPerformance, "JIT recompiler", Both, [Jit], One("gba_jit"), ["gba jit", "faster", "dynarec"], PerGame;
    GbaCompiledCode: GbaPerformance, "Compiled game code", Both, [Jit], Action, ["jit cache", "clear jit", "compiled code"];
    LightSensor: Solar, "Use light sensor", Both, [], One("gimmicks.light_source"), ["boktai", "ambient light", "this device's light sensor"];
    Sunlight: Solar, "Sunlight", Both, [], One("gimmicks.light"), ["boktai", "light level"], PerGame;

    SmallCorner: DsScreens, "Small screen corner", Both, [], One("ds_screens.corner"), ["picture in picture"], PerGame;
    SmallSize: DsScreens, "Small screen size", Both, [], One("ds_screens.small"), ["picture in picture"], PerGame;
    Gap: DsScreens, "Gap", Both, [], One("ds_screens.gap"), ["space between screens"], PerGame;
    Rotation: DsScreens, "Rotation", Both, [], One("ds_screens.rotation"), ["rotate", "sideways"], PerGame;
    TwoTabs: DsScreens, "Screens in two tabs", Desktop, [], One("ds_screens.separate_tabs"), ["separate tabs"];
    Resolution3d: Ds3d, "3D resolution", Both, [NotWeb], One("ds_3d.resolution"), ["upscale", "internal resolution"], PerGame;
    SmoothEdges: Ds3d, "Anti-aliasing", Both, [NotWeb], One("ds_3d.smooth_edges"), ["smooth 3d edges", "antialiasing", "msaa", "jaggies"], PerGame;
    TextureFilter: Ds3d, "Texture filter", Both, [NotWeb], One("ds_3d.texture_filter"), ["texture filtering", "smooth textures", "nearest", "bilinear", "bicubic", "xbr", "pixelated", "soft", "smooth", "rounded"], PerGame;
    DistantTextures: Ds3d, "Anisotropic filtering", Both, [NotWeb], One("ds_3d.distant_textures"), ["fix distant shimmer", "less shimmer in the distance", "shimmering", "far texture detail", "distant textures", "far textures", "mipmaps", "mipmapping", "trilinear", "blurred", "sharp", "sharper", "sharpest"], PerGame;
    Nickname: DsUser, "Nickname", Both, [], One("ds_user.nickname"), ["user name"];
    Birthday: DsUser, "Birthday", Both, [], One("ds_user.birthday"), ["birth date"];
    Language: DsUser, "Language", Both, [], One("ds_user.language"), ["ds language"], PerGame;
    Colour: DsUser, "Colour", Both, [], One("ds_user.favorite_color"), ["favourite colour", "favorite color"];
    Clock: DsUser, "Clock", Both, [], One("ds_user.clock"), ["date", "time", "rtc"];
    Jit: DsPerformance, "JIT recompiler", Both, [Jit], One("ds_jit"), ["ds jit", "faster", "dynarec"], PerGame;
    CompiledCode: DsPerformance, "Compiled game code", Both, [Jit], Action, ["jit cache", "clear jit", "compiled code"];
    Arm9Bios: DsSystem, "ARM9 BIOS", Both, [], Action, ["ds bios"];
    Arm7Bios: DsSystem, "ARM7 BIOS", Both, [], Action, ["ds bios"];
    Firmware: DsSystem, "Firmware", Both, [], Action, ["ds firmware"];
    KeepFirmware: DsSystem, "Use firmware user", Both, [], One("ds_user.keep_firmware_settings"), ["firmware keeps its own settings", "firmware settings", "keep its own settings"];
    OnlineService: DsSystem, "Online service", Both, [DsWifi], One("ds_online.service"), ["wiimmfi", "kaeru", "wi-fi", "wfc"];
    DsDirect: DsSystem, "Connect without the relay", Both, [DsWifi], One("ds_online.direct"), ["wi-fi", "wfc"];
    Dns: DsSystem, "DNS server", Both, [DsWifi], One("ds_online.dns"), ["wi-fi", "wfc"];
    StepsInGame: Walker, "Count in-game steps", Both, [], One("walker.in_game"), ["steps in-game", "steps walked"];
    PhoneSteps: Walker, "Count phone steps", Both, [Android], One("walker.phone"), ["pedometer", "step counter"];
    Microphone: DsMicrophone, "Input", Both, [Nds], One("microphone"), ["mic", "microphone", "voice", "blow", "input device"];
    PushToTalk: DsMicrophone, "Push to talk", Both, [Nds], One("push_to_talk"), ["mic", "microphone", "hold to talk", "talk key"];
    TalkButton: DsMicrophone, "Talk button on screen", Phone, [Nds], One("talk_button"), ["mic", "microphone", "push to talk", "on-screen button"];

    Theme: Look, "Theme", Both, [], One("ui_theme"), ["dark mode", "light mode", "same as system"];
    AppLayout: Look, "App layout", Both, [], One("layout"), ["touch layout", "desktop layout"];
    UiScale: Look, "UI scale", Both, [], PerSurface { desktop: &["ui_scale"], phone: &["touch.ui_scale"] }, ["ui size", "text size", "zoom"];
    Fullscreen: Look, "Fullscreen", Both, [NativeDesktop], One("fullscreen"), ["full screen", "window"];
    HidePointer: Look, "Hide the pointer over games", Both, [NotAndroid], One("hide_pointer"), ["mouse", "cursor", "over the game"];
    Covers: Library, "Game covers", Both, [], One("covers"), ["box art", "artwork", "thumbnails"];
    Subfolders: Library, "Look in subfolders", Both, [NotWeb], One("library_depth"), ["scan depth", "folders", "library scan"];
    AchievementProgress: Achievements, "Progress in the library", Both, [NotWeb], One("achievements.in_library"), ["achievements", "retroachievements", "achievement progress"];
    DiscordActivity: Discord, "Show activity", Both, [NativeDesktop], One("discord.enabled"), ["discord", "rich presence"];
    DiscordGame: Discord, "Game name", Both, [NativeDesktop], One("discord.game"), ["discord"];
    DiscordDetails: Discord, "Details", Both, [NativeDesktop], One("discord.details"), ["discord"];
    DiscordTime: Discord, "Session time", Both, [NativeDesktop], One("discord.play_time"), ["discord", "play time", "elapsed"];
    DiscordShot: Discord, "Screenshot", Both, [NativeDesktop], One("discord.screenshot"), ["discord"];
    DiscordEvery: Discord, "Shot every", Both, [NativeDesktop], One("discord.every"), ["discord", "screenshot every", "time between screenshots"];
    DiscordDsScreens: Discord, "DS screens", Both, [NativeDesktop, Nds], One("discord.screens"), ["discord", "ds screenshot", "ds screens in the screenshot", "both screens", "top screen", "bottom screen"];
    CheckUpdates: Updates, "Check at start", Both, [NotWeb], One("check_updates"), ["check for updates", "update", "version"];

    Account: Account, "Account", Both, [], Action, ["sign in", "log in", "create account", "join", "invite code"];
    Server: Account, "Server", Both, [], One("online.address"), ["relay address", "host"];
    SwitchAccount: Account, "Switch account", Desktop, [], One("online.relays"), ["accounts", "add account"];
    DisplayName: Account, "Display name", Desktop, [], Server, ["user name"];
    OnlineId: Account, "Online ID", Desktop, [], Action, ["friend code", "copy id"];
    RelayId: Account, "Relay ID", Desktop, [], Action, ["relay"];
    RelayKey: Account, "Relay key", Desktop, [], Action, ["fingerprint"];
    TwoFactor: Account, "Two-factor sign-in", Desktop, [], Server, ["2fa", "recovery codes", "authenticator"];
    ShowPlaying: Friends, "Friends see what I play", Desktop, [], One("online.show_playing"), ["presence", "activity", "what i'm playing"];
    Typing: Friends, "Typing indicators", Both, [], One("online.typing_indicators"), ["chat"];
    TypingText: Friends, "Typing text", Both, [], One("online.typing_text"), ["chat", "typing message"];
    FocusPopups: Friends, "Popups in focus mode", Desktop, [], One("online.focus_popups"), ["messages", "do not disturb", "invites"];
    AllowNotifications: Notifications, "Allow notifications", Both, [], Action, ["turn on notifications", "push", "notification status"];
    FriendRequests: Notifications, "Friend requests", Both, [], One("online.notify_friend_requests"), ["notifications"];
    PlayInvites: Notifications, "Play invites", Both, [], One("online.notify_invites"), ["notifications"];
    Messages: Notifications, "Messages", Both, [], One("online.notify_chats"), ["notifications", "chat"];
    AndroidNotifications: Notifications, "Android notification settings", Both, [Android], Action, ["system settings"];
    InputDelay: PlayTogether, "Input delay", Both, [], One("online.input_delay"), ["lag", "netplay"];
    Direct: PlayTogether, "Direct connection", Both, [], One("online.direct"), ["peer to peer", "p2p", "netplay"];
    CloudSaves: Cloud, "Cloud saves", Both, [], One("online.cloud_saves"), ["sync saves", "backup"];
    SyncSettings: Cloud, "Sync settings", Both, [], One("online.sync_settings"), ["settings sync"];
    NetworkSaves: Network, "Saves on this network", Both, [NotWeb], One("online.network_saves"), ["lan", "wifi", "local sync", "nearby devices"];
    NetworkDevices: Network, "Devices on this network", Both, [NotWeb], One("online.network_allowed"), ["nearby devices", "sync saves"];
    KeepSaves: Elsewhere, "Download saves for", Both, [], One("online.keep_saves"), ["keep saves", "download saves", "other games"];
    SavesElsewhere: Elsewhere, "Saves elsewhere", Both, [], Action, ["get saves", "other devices"];
    Devices: Devices, "Devices", Desktop, [], One("online.device_name"), ["rename this device", "remove a device"];
    Storage: Storage, "Cloud storage", Both, [], Server, ["cloud storage", "space used", "delete files"];
    LeaveRelay: Leave, "Leave this relay", Desktop, [], One("online.joined"), ["sign out", "log out"];
    DeleteAccount: Leave, "Delete account", Desktop, [], Server, ["remove account"];

    SpaceUsed: YourData, "Space used", Both, [], Action, ["storage", "disk space", "size", "saves size"];
    Export: YourData, "Export", Both, [], Action, ["backup", "export data"];
    Import: YourData, "Import", Both, [], Action, ["restore", "import data"];
    DeleteEverything: YourData, "Delete everything", Both, [], Action, ["reset", "wipe", "erase"];
    ClearCache: Cache, "Clear cache", Both, [], Action, ["cache", "free space", "covers", "temporary files"];

    WhatsNew: About, "What's new", Both, [], Action, ["changelog", "release notes"];
    FeaturesAndPlans: About, "Features and plans", Both, [], Action, ["roadmap"];
    ReportProblem: About, "Report a problem", Both, [], Action, ["bug", "issue", "feedback"];
    DesktopApp: About, "Desktop app", Both, [Web], Action, ["download"];
    AndroidApp: About, "Android app", Both, [Web], Action, ["download", "phone"];
    AllVersions: About, "All versions", Both, [NotWeb], Action, ["releases", "download"];
    Licences: About, "Licences", Both, [], Action, ["licenses", "open source"];
    SourceCode: About, "Source code", Both, [], Action, ["github", "repository", "gpl"];
    DevTools: About, "Dev tools", Both, [], One("dev_mode"), ["developer", "debug"];

    RandomRam: Developer, "Random power-on RAM", Both, [], One("randomized_ram"), ["random memory", "randomized"];
    DsFiles: Developer, "DS files", Both, [Nds], Action, ["nds files"];
}

impl RowId {
    pub fn parent(self) -> Option<RowId> {
        match self {
            Self::GridLines | Self::GridCorners => Some(Self::LcdGrid),
            Self::GhostingStrength => Some(Self::Ghosting),
            Self::HudOpacity => Some(Self::ShowHuds),
            Self::TouchSize
            | Self::TouchOpacity
            | Self::TouchScaling
            | Self::TouchLandscape
            | Self::TouchHidden
            | Self::LeftHanded
            | Self::Haptics => Some(Self::TouchShow),
            Self::ConfirmButton | Self::ButtonIcons | Self::AroundTheGame => {
                Some(Self::BigScreenMode)
            }
            Self::DiscordGame | Self::DiscordDetails | Self::DiscordTime | Self::DiscordShot => {
                Some(Self::DiscordActivity)
            }
            Self::DiscordEvery | Self::DiscordDsScreens => Some(Self::DiscordShot),
            Self::TypingText => Some(Self::Typing),
            _ => None,
        }
    }

    pub fn depth(self) -> usize {
        self.parent().map_or(0, |parent| parent.depth() + 1)
    }

    fn spec(self) -> &'static Spec {
        &SPECS[self as usize]
    }

    pub fn group(self) -> GroupId {
        self.spec().group
    }

    pub fn page(self) -> SettingsTab {
        self.group().page()
    }

    pub fn label(self, surface: Surface) -> &'static str {
        let spec = self.spec();
        match surface {
            Surface::Desktop => spec.label,
            Surface::Phone => spec.phone_label.unwrap_or(spec.label),
        }
    }

    pub fn keywords(self) -> &'static [&'static str] {
        self.spec().keywords
    }

    pub fn surfaces(self) -> &'static [Surface] {
        self.spec().on.surfaces()
    }

    pub fn available(self, dev_mode: bool) -> bool {
        self.page().available(dev_mode) && self.spec().needs.iter().all(|need| need.met())
    }

    pub fn field(self) -> Field {
        self.spec().field
    }

    pub fn per_game(self) -> bool {
        self.spec().per_game
    }

    pub fn shown_on(self, surface: Surface, dev_mode: bool) -> bool {
        self.surfaces().contains(&surface)
            && self.available(dev_mode)
            && !matches!(self.field(), Pending(_))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, EnumIter)]
pub enum ThisGameRow {
    PlayOn,
    GameClock,
    RecenterTilt,
    CameraView,
    CentreCameraView,
    Printer,
    Cheats,
    SaveGameReport,
}

impl ThisGameRow {
    pub fn label(self) -> &'static str {
        match self {
            Self::PlayOn => "Play on",
            Self::GameClock => "Game clock",
            Self::RecenterTilt => "Recenter tilt",
            Self::CameraView => "Camera view",
            Self::CentreCameraView => "Centre camera view",
            Self::Printer => "Game Boy Printer",
            Self::Cheats => "Cheats",
            Self::SaveGameReport => "Save game report",
        }
    }

    pub fn field(self) -> Field {
        match self {
            Self::PlayOn => One("per_game"),
            Self::GameClock => One("gimmicks.clock_offsets"),
            Self::CameraView | Self::CentreCameraView => One("gimmicks.camera_view"),
            Self::Printer => One("gimmicks.printers"),
            Self::Cheats => One("cheats"),
            Self::RecenterTilt | Self::SaveGameReport => Action,
        }
    }
}
