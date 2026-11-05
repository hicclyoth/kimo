//! Windows breakage found on the AU_0074 pilot (Nov 2026): each test was written red on Windows 10 against kimo 8.12.1.
use kimo_core::commands::{dispatch, Session};
use serde_json::{json, Value};

fn call(ss: &mut Session, name: &str, a: Value) -> Value { dispatch(ss, name, &a).unwrap_or_else(|e| panic!("{name} {a}: {e}")) }

/// A missing font fell back to a Linux path, so every text clip using it crashed with "cannot find the path" on Windows.
#[test] fn fallback_font_is_a_real_file_on_this_os() {
    let (file, sub) = kimo_core::text::resolve_font_info("NoSuchFontFamilyAtAll");
    assert!(sub, "a made-up family must count as substituted");
    assert!(std::path::Path::new(&file).is_file(), "fallback font must exist on this OS, got {file}");
}

#[test] fn check_environment_reports_a_fallback_font_that_exists() {
    let mut ss = Session::new(); let r = call(&mut ss, "check_environment", json!({}));
    assert_eq!(r["fonts"]["fallback"]["present"], true, "{}", r["fonts"]);
}

/// fetch.sh needs bash, so Windows had no way to get the preset fonts.
#[test] fn fonts_have_a_powershell_fetch_step() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let ps = std::fs::read_to_string(root.join("fonts/fetch.ps1")).expect("fonts/fetch.ps1 must exist");
    let sh = std::fs::read_to_string(root.join("fonts/fetch.sh")).unwrap();
    for line in sh.lines().filter(|l| l.ends_with(".ttf")) { assert!(ps.contains(line), "fetch.ps1 is missing {line}"); }
}

/// The hints told Windows users to run apt-get.
#[test] fn setup_hints_fit_this_os() {
    let mut ss = Session::new(); std::env::set_var("KIMO_FONTS", std::env::temp_dir().join("kimo_no_fonts_here"));
    let r = call(&mut ss, "check_environment", json!({})); let fixes = r["fixes"].to_string();
    if cfg!(windows) { assert!(!fixes.contains("apt-get") && !fixes.contains("export KIMO_FONTS"), "{fixes}"); }
}

/// std::fs::canonicalize gives \\?\W:\... on Windows; that reached ffmpeg through the project's base dir and lint_edit's proxy
/// failed with "Invalid argument".
#[test] fn project_base_dir_has_no_verbatim_prefix() {
    let f = std::env::temp_dir().join("kimo_wp_proj.kimo");
    let d = kimo_core::deps::dir_of(f.to_str().unwrap()).unwrap();
    let s = d.to_string_lossy();
    assert!(!s.starts_with(r"\\?\"), "ffmpeg cannot use {s}");
    assert!(d.is_dir(), "{s}");
}
