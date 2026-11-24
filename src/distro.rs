//! Linux distribution detection, so error messages and diagnostics can suggest the right
//! package-manager command on Arch-, Debian- and Fedora-based systems (and openSUSE).

use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    Arch,
    Debian,
    Fedora,
    Suse,
    Unknown,
}

/// Packages Rustym cares about, named independently of any distribution.
#[derive(Clone, Copy)]
#[allow(dead_code)]
pub enum Pkg {
    Mpv,
    YtDlp,
    Deno,
    Ffmpeg,
    Pulse,
    Webkit,
    Coreutils,
}

fn read_os_release() -> String {
    std::fs::read_to_string("/etc/os-release")
        .or_else(|_| std::fs::read_to_string("/usr/lib/os-release"))
        .unwrap_or_default()
}

fn field(text: &str, key: &str) -> String {
    text.lines()
        .find_map(|l| l.strip_prefix(&format!("{key}=")))
        .map(|v| v.trim().trim_matches('"').trim_matches('\'').to_lowercase())
        .unwrap_or_default()
}

/// The distribution family, from `ID` and `ID_LIKE` in /etc/os-release (so Manjaro, CachyOS,
/// EndeavourOS, Ubuntu, Mint, Pop!_OS, Fedora, Nobara, RHEL clones ... all resolve correctly).
pub fn family() -> Family {
    static F: OnceLock<Family> = OnceLock::new();
    *F.get_or_init(|| {
        let t = read_os_release();
        let ids = format!("{} {}", field(&t, "ID"), field(&t, "ID_LIKE"));
        let has = |w: &str| ids.split_whitespace().any(|x| x == w);
        if has("arch") || has("archlinux") || has("manjaro") || has("cachyos") || has("endeavouros") {
            Family::Arch
        } else if has("debian") || has("ubuntu") || has("linuxmint") || has("raspbian") {
            Family::Debian
        } else if has("fedora") || has("rhel") || has("centos") || has("rocky") || has("almalinux") {
            Family::Fedora
        } else if has("suse") || has("opensuse") || has("sles") {
            Family::Suse
        } else {
            Family::Unknown
        }
    })
}

/// Human-readable distribution name for the diagnostics log.
pub fn pretty_name() -> String {
    let t = read_os_release();
    let n = field(&t, "PRETTY_NAME");
    if n.is_empty() { "unknown distribution".into() } else { n }
}

fn pkg_name(f: Family, p: Pkg) -> &'static str {
    use Family::*;
    use Pkg::*;
    match (f, p) {
        (_, Mpv) => "mpv",
        (_, Ffmpeg) => "ffmpeg",
        (_, Coreutils) => "coreutils",
        (Arch, YtDlp) | (Fedora, YtDlp) | (Suse, YtDlp) => "yt-dlp",
        (Arch, Deno) | (Fedora, Deno) => "deno",
        (Arch, Pulse) => "libpulse",
        (Debian, Pulse) => "pulseaudio-utils",
        (Fedora, Pulse) | (Suse, Pulse) => "pulseaudio-utils",
        (Arch, Webkit) => "webkit2gtk-4.1",
        (Debian, Webkit) => "libwebkit2gtk-4.1-0",
        (Fedora, Webkit) => "webkit2gtk4.1",
        (Suse, Webkit) => "libwebkit2gtk-4_1-0",
        _ => "",
    }
}

fn installer(f: Family) -> &'static str {
    match f {
        Family::Arch => "sudo pacman -S",
        Family::Debian => "sudo apt install",
        Family::Fedora => "sudo dnf install",
        Family::Suse => "sudo zypper install",
        Family::Unknown => "",
    }
}

/// A copy-paste command that installs `pkgs`, or a generic sentence when the distribution is
/// unknown. yt-dlp and deno get a note where the distro package is missing or usually stale.
pub fn install_hint(pkgs: &[Pkg]) -> String {
    let f = family();
    let mut names: Vec<&str> = Vec::new();
    let mut extra: Vec<&str> = Vec::new();
    for p in pkgs {
        let n = pkg_name(f, *p);
        match (p, n) {
            (Pkg::YtDlp, _) if f == Family::Debian => {
                extra.push("yt-dlp: use the current release (apt's copy is usually outdated): download it from github.com/yt-dlp/yt-dlp/releases/latest to ~/.local/bin/yt-dlp, or run: yt-dlp -U")
            }
            (Pkg::Deno, "") => extra.push("deno: install from https://deno.com (curl -fsSL https://deno.land/install.sh | sh)"),
            (_, "") => {}
            (_, n) => names.push(n),
        }
    }
    let mut out = String::new();
    if f == Family::Unknown {
        out.push_str("install it with your package manager (mpv, yt-dlp, ffmpeg, deno)");
    } else if !names.is_empty() {
        out.push_str(&format!("{} {}", installer(f), names.join(" ")));
    }
    for e in extra {
        if !out.is_empty() {
            out.push_str("; ");
        }
        out.push_str(e);
    }
    out
}

/// Command that updates the whole system (used when mpv is out of sync with its libraries).
pub fn upgrade_hint() -> &'static str {
    match family() {
        Family::Arch => "sudo pacman -Syu",
        Family::Debian => "sudo apt update && sudo apt full-upgrade",
        Family::Fedora => "sudo dnf upgrade --refresh",
        Family::Suse => "sudo zypper dup",
        Family::Unknown => "update your system with your package manager",
    }
}

/// Shell snippet printing the installed versions of the main dependencies (for the log).
pub fn query_installed_cmd() -> &'static str {
    match family() {
        Family::Arch => "pacman -Q mpv libjxl yt-dlp deno yt-dlp-ejs python ffmpeg 2>&1",
        Family::Debian => "dpkg-query -W -f='${Package} ${Version}\\n' mpv yt-dlp ffmpeg deno 2>&1",
        Family::Fedora | Family::Suse => "rpm -q mpv yt-dlp ffmpeg deno ffmpeg-free 2>&1",
        Family::Unknown => "true",
    }
}

/// Hint for finding which package provides a missing shared library.
pub fn missing_lib_hint() -> String {
    match family() {
        Family::Arch => format!("{} first. Still missing? find the owner with: sudo pacman -Fy ; pacman -F <libname>  then sudo pacman -S <package>", upgrade_hint()),
        Family::Debian => format!("{} first. Still missing? apt-file search <libname>, then sudo apt install <package>", upgrade_hint()),
        Family::Fedora => format!("{} first. Still missing? sudo dnf provides '*/<libname>', then sudo dnf install <package>", upgrade_hint()),
        _ => format!("{} first, then install the package that provides the missing library", upgrade_hint()),
    }
}

/// Desktop environment (for the diagnostics log and tray advice).
pub fn desktop() -> String {
    std::env::var("XDG_CURRENT_DESKTOP").or_else(|_| std::env::var("DESKTOP_SESSION")).unwrap_or_else(|_| "unknown".into())
}

/// Tray advice for the current desktop.
pub fn tray_advice() -> &'static str {
    let d = desktop().to_lowercase();
    if d.contains("gnome") || d.contains("unity") || d.contains("ubuntu") {
        match family() {
            Family::Debian => "GNOME has no tray by default: sudo apt install gnome-shell-extension-appindicator, then enable it in Extensions.",
            Family::Fedora => "GNOME has no tray by default: sudo dnf install gnome-shell-extension-appindicator, then enable it in Extensions.",
            Family::Arch => "GNOME has no tray by default: sudo pacman -S gnome-shell-extension-appindicator, then enable it in Extensions.",
            _ => "GNOME has no tray by default: install the AppIndicator / KStatusNotifierItem extension and enable it.",
        }
    } else if d.contains("kde") || d.contains("plasma") || d.contains("lxqt") || d.contains("xfce") || d.contains("mate") || d.contains("cinnamon") || d.contains("budgie") || d.contains("deepin") {
        "Your desktop normally has a tray: enable the system tray / notification area panel applet."
    } else if d.contains("hypr") || d.contains("sway") || d.contains("river") || d.contains("niri") || d.contains("i3") {
        "Add a StatusNotifier-capable tray to your bar (waybar `tray` module, or a tray applet)."
    } else {
        "Install a StatusNotifierItem-capable tray (KDE has one built in; GNOME needs the AppIndicator extension)."
    }
}
