use std::process::Command;
use std::sync::OnceLock;

// AppRun and its hooks set these without pointing them at the bundle; GTK_THEME is forced to
// Adwaita by the gtk hook, so the value we see is never the user's own.
const SET_BY_APPRUN: &[&str] = &[
    "APPDIR",
    "APPIMAGE",
    "ARGV0",
    "OWD",
    "GTK_THEME",
    "PYTHONDONTWRITEBYTECODE",
    "GST_REGISTRY_REUSE_PLUGIN_SCANNER",
];

type EnvFix = Vec<(String, Option<String>)>;

// Must run before the webview starts. Points fontconfig at the bundled fonts.conf, which hides
// the COLRv1 fonts that crash the bundled WebKit. Pointing inside the bundle also keeps it out
// of launched programs: restore_system_env drops it like any other bundle path.
pub(crate) fn use_bundled_fontconfig() {
    let appdir = std::env::var("APPDIR").ok();
    let own = std::env::var_os("FONTCONFIG_FILE").is_some();
    if let Some(file) = bundled_fontconfig(appdir.as_deref(), own) {
        std::env::set_var("FONTCONFIG_FILE", file);
    }
}

fn bundled_fontconfig(appdir: Option<&str>, user_has_own: bool) -> Option<String> {
    let appdir = appdir.filter(|dir| !dir.is_empty() && !user_has_own)?;
    Some(format!("{}/usr/share/panorama/fonts.conf", appdir.trim_end_matches('/')))
}

// Inside an AppImage, AppRun points LD_LIBRARY_PATH, PATH, PYTHONHOME and friends at the bundle so
// the webview loads its own libraries. Anything we launch for the user (shells, git, xdg-open)
// must see the system's instead, or it mixes the bundle's libraries with the system's.
pub(crate) fn restore_system_env(cmd: &mut Command) {
    static FIX: OnceLock<EnvFix> = OnceLock::new();
    let fix = FIX.get_or_init(|| match std::env::var("APPDIR") {
        Ok(appdir) => env_fix(&appdir, std::env::vars()),
        Err(_) => Vec::new(),
    });
    for (key, value) in fix {
        match value {
            Some(value) => cmd.env(key, value),
            None => cmd.env_remove(key),
        };
    }
}

fn env_fix(appdir: &str, vars: impl Iterator<Item = (String, String)>) -> EnvFix {
    if appdir.is_empty() {
        return Vec::new();
    }
    vars.filter_map(|(key, value)| {
        if SET_BY_APPRUN.contains(&key.as_str()) {
            return Some((key, None));
        }
        if !value.contains(appdir) {
            return None;
        }
        let kept: Vec<&str> = value
            .split(':')
            .filter(|part| !part.is_empty() && !part.starts_with(appdir))
            .collect();
        Some((key, (!kept.is_empty()).then(|| kept.join(":"))))
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const APPDIR: &str = "/tmp/.mount_panoraXYZ";

    fn fix(vars: &[(&str, &str)]) -> EnvFix {
        let vars = vars.iter().map(|(k, v)| (k.to_string(), v.to_string()));
        let mut out = env_fix(APPDIR, vars);
        out.sort();
        out
    }

    fn set(key: &str, value: &str) -> (String, Option<String>) {
        (key.into(), Some(value.into()))
    }

    fn unset(key: &str) -> (String, Option<String>) {
        (key.into(), None)
    }

    #[test]
    fn drops_bundle_entries_and_keeps_the_users() {
        let out = fix(&[
            ("PATH", "/tmp/.mount_panoraXYZ/usr/bin/:/tmp/.mount_panoraXYZ/usr/sbin/:/home/me/.cargo/bin:/usr/bin"),
            ("XDG_DATA_DIRS", "/tmp/.mount_panoraXYZ/usr/share/:/usr/share:/var/lib/flatpak/exports/share"),
        ]);
        assert_eq!(
            out,
            [set("PATH", "/home/me/.cargo/bin:/usr/bin"), set("XDG_DATA_DIRS", "/usr/share:/var/lib/flatpak/exports/share")]
        );
    }

    #[test]
    fn unsets_vars_that_only_pointed_at_the_bundle() {
        let out = fix(&[
            ("LD_LIBRARY_PATH", "/tmp/.mount_panoraXYZ/usr/lib/:/tmp/.mount_panoraXYZ/usr/lib/x86_64-linux-gnu/:"),
            ("PYTHONHOME", "/tmp/.mount_panoraXYZ/usr/"),
            ("GDK_PIXBUF_MODULE_FILE", "/tmp/.mount_panoraXYZ//usr/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache"),
        ]);
        assert_eq!(out, [unset("GDK_PIXBUF_MODULE_FILE"), unset("LD_LIBRARY_PATH"), unset("PYTHONHOME")]);
    }

    #[test]
    fn unsets_what_apprun_sets_and_leaves_the_rest() {
        let out = fix(&[
            ("APPIMAGE", "/home/me/Applications/Panorama.AppImage"),
            ("GTK_THEME", "Adwaita:dark"),
            ("PYTHONDONTWRITEBYTECODE", "1"),
            ("GST_REGISTRY_REUSE_PLUGIN_SCANNER", "no"),
            ("HOME", "/home/me"),
            ("SHELL", "/bin/bash"),
        ]);
        assert_eq!(
            out,
            [
                unset("APPIMAGE"),
                unset("GST_REGISTRY_REUSE_PLUGIN_SCANNER"),
                unset("GTK_THEME"),
                unset("PYTHONDONTWRITEBYTECODE"),
            ]
        );
    }

    #[test]
    fn fontconfig_points_at_the_bundle_only_inside_an_appimage() {
        assert_eq!(
            bundled_fontconfig(Some("/tmp/.mount_panoraXYZ/"), false).as_deref(),
            Some("/tmp/.mount_panoraXYZ/usr/share/panorama/fonts.conf")
        );
        assert_eq!(bundled_fontconfig(None, false), None);
        assert_eq!(bundled_fontconfig(Some(""), false), None);
        assert_eq!(bundled_fontconfig(Some(APPDIR), true), None);
    }

    #[test]
    fn launched_programs_do_not_inherit_the_bundled_fontconfig() {
        let out = fix(&[("FONTCONFIG_FILE", "/tmp/.mount_panoraXYZ/usr/share/panorama/fonts.conf")]);
        assert_eq!(out, [unset("FONTCONFIG_FILE")]);
    }

    #[test]
    fn bundled_fontconfig_is_packaged_and_hides_colrv1() {
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let src = &conf["bundle"]["linux"]["appimage"]["files"]["/usr/share/panorama/fonts.conf"];
        assert_eq!(src, "appimage/fonts.conf");
        let fonts = include_str!("../appimage/fonts.conf");
        assert!(fonts.contains("<include ignore_missing=\"yes\">/etc/fonts/fonts.conf</include>"));
        assert!(fonts.contains("<glob>*COLRv1*</glob>"));
    }

    #[test]
    fn no_appdir_changes_nothing() {
        let vars = [("PATH".to_string(), "/usr/bin".to_string())];
        assert!(env_fix("", vars.into_iter()).is_empty());
    }
}
