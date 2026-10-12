mod support;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn fixture(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("valid clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "uni-init-{}-{}-{}",
        name,
        std::process::id(),
        nonce
    ));
    std::fs::create_dir_all(&dir).expect("fixture created");
    dir
}

fn init(dir: &Path, args: &[&str]) {
    let output = Command::new(support::bin())
        .arg("init")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("uni init runs");
    assert!(
        output.status.success(),
        "init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn init_profiles_create_trusted_registry_templates() {
    let scenarios: &[(&str, &[&str], &[&str])] = &[
        (
            "default",
            &[],
            &["cargo check", "cargo test", "cargo build"],
        ),
        (
            "node",
            &["--profile", "node"],
            &["npm run typecheck", "npm test", "npm run build"],
        ),
        (
            "nodejs-alias",
            &["--profile", "nodejs"],
            &["npm run typecheck", "npm test"],
        ),
        (
            "python",
            &["--profile", "python"],
            &["python -m compileall -q .", "python -m unittest discover"],
        ),
    ];
    for (name, args, commands) in scenarios {
        let dir = fixture(name);
        init(&dir, args);
        let config =
            std::fs::read_to_string(dir.join(".uni/config.toml")).expect("registry written");
        for command in *commands {
            assert!(config.contains(command), "{name} did not include {command}");
        }
        for subdir in [
            "contracts",
            "evidence",
            "decisions",
            "artifacts",
            "policies",
        ] {
            assert!(dir.join(".uni").join(subdir).is_dir());
        }
        assert!(dir.join("uni/intents").is_dir());
        std::fs::remove_dir_all(dir).expect("fixture removed");
    }
}

#[test]
fn init_never_overwrites_existing_trusted_registry() {
    let dir = fixture("existing-registry");
    init(&dir, &["--profile", "node"]);
    let config = dir.join(".uni/config.toml");
    std::fs::write(&config, "[verifiers]\n\"owner.check\" = \"true\"\n")
        .expect("custom trusted registry");
    init(&dir, &["--profile", "python"]);
    assert_eq!(
        std::fs::read_to_string(config).expect("registry remains"),
        "[verifiers]\n\"owner.check\" = \"true\"\n"
    );
    std::fs::remove_dir_all(dir).expect("fixture removed");
}

#[test]
fn init_rejects_unknown_profiles() {
    let dir = fixture("unknown-profile");
    let output = Command::new(support::bin())
        .args(["init", "--profile", "unknown"])
        .current_dir(&dir)
        .output()
        .expect("uni init runs");
    assert!(!output.status.success());
    assert!(!dir.join(".uni").exists());
    std::fs::remove_dir_all(dir).expect("fixture removed");
}
