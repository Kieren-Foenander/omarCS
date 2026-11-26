use std::{
    fs,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn bootstrap_scans_locally_without_steam_or_systemd() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("omarcs-bootstrap-{}-{stamp}", std::process::id()));
    let data = root.join("data");
    let vrf = data.join("omarcs/vrf");
    fs::create_dir_all(&vrf).unwrap();
    for name in ["Source2Viewer-CLI", "libSkiaSharp.so", "libspirv-cross.so"] {
        fs::write(vrf.join(name), "").unwrap();
    }
    fs::write(vrf.join(".version"), "20.0\n").unwrap();
    // Existing helper stubs make the old automatic setup fail immediately,
    // without downloading anything or connecting to the real Steam session.
    let helper = data.join("omarcs/boiler-writter");
    fs::create_dir_all(&helper).unwrap();
    for name in ["boiler-writter", "libsteam_api.so", "steam_appid.txt"] {
        fs::write(helper.join(name), "").unwrap();
    }
    let output = Command::new(env!("CARGO_BIN_EXE_omarcs"))
        .arg("bootstrap")
        .env("HOME", &root)
        .env("XDG_DATA_HOME", &data)
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(root.join("state/omarcs/summary.json").is_file());
    assert!(
        !root
            .join(".config/systemd/user/omarcs-autofetch.service")
            .exists()
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn steam_refresh_is_explicit_and_failed_imports_remain_queued() {
    use std::os::unix::fs::PermissionsExt;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("omarcs-once-{}-{stamp}", std::process::id()));
    let data = root.join("data");
    let helper = data.join("omarcs/boiler-writter");
    let state = root.join("state/omarcs");
    fs::create_dir_all(&helper).unwrap();
    fs::create_dir_all(&state).unwrap();
    let url = "https://replay123.valve.net/730/test.dem.bz2";
    let binary = helper.join("boiler-writter");
    fs::write(
        &binary,
        format!("#!/bin/sh\nprintf 'query\\n' >> calls\nprintf '%s' '{url}' > \"$1\"\n"),
    )
    .unwrap();
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
    for name in ["libsteam_api.so", "steam_appid.txt"] {
        fs::write(helper.join(name), "").unwrap();
    }
    fs::write(
        state.join("autofetch.json"),
        serde_json::json!({"knownUrls": [url]}).to_string(),
    )
    .unwrap();
    let run = |steam: bool| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_omarcs"));
        command.args(["refresh", "--quiet"]);
        if steam {
            command.arg("--steam");
        }
        command
            .env("HOME", &root)
            .env("XDG_DATA_HOME", &data)
            .env("XDG_CONFIG_HOME", root.join("config"))
            .env("XDG_STATE_HOME", root.join("state"))
            .env("XDG_CACHE_HOME", root.join("cache"))
            .env("PATH", "")
            .output()
            .unwrap()
    };
    assert!(run(false).status.success());
    assert!(!helper.join("calls").exists());
    let output = run(true);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read_to_string(helper.join("calls")).unwrap(), "query\n");
    assert!(!root.join(".config/systemd").exists());

    // A cached, malformed demo exercises the real import subprocess without
    // network access. The queued work must survive its failure.
    let demos = data.join("omarcs/demos");
    fs::create_dir_all(&demos).unwrap();
    fs::write(demos.join("test.dem"), "invalid demo").unwrap();
    fs::write(state.join("autofetch.json"), "{}").unwrap();
    let output = run(true);
    assert!(!output.status.success());
    let saved: serde_json::Value =
        serde_json::from_slice(&fs::read(state.join("autofetch.json")).unwrap()).unwrap();
    assert_eq!(saved["queue"], serde_json::json!([url]));
    assert_eq!(saved["status"], "error");
    assert_eq!(
        fs::read_to_string(helper.join("calls")).unwrap(),
        "query\nquery\n"
    );
    assert!(demos.join("test.dem").exists());
    fs::remove_dir_all(root).unwrap();
}
