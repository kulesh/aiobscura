use aiobscura_core::{Assistant, Database, SessionFilter};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

struct CliTestEnv {
    _temp_dir: TempDir,
    home: PathBuf,
    xdg_data: PathBuf,
    xdg_config: PathBuf,
    xdg_state: PathBuf,
}

impl CliTestEnv {
    fn new() -> Self {
        let temp_dir = TempDir::new().expect("failed to create temp dir");
        let base = temp_dir.path().to_path_buf();
        let home = base.join("home");
        let xdg_data = base.join("xdg-data");
        let xdg_config = base.join("xdg-config");
        let xdg_state = base.join("xdg-state");

        fs::create_dir_all(&home).expect("failed to create HOME");
        fs::create_dir_all(&xdg_data).expect("failed to create XDG_DATA_HOME");
        fs::create_dir_all(&xdg_config).expect("failed to create XDG_CONFIG_HOME");
        fs::create_dir_all(&xdg_state).expect("failed to create XDG_STATE_HOME");

        seed_codex_fixture(&home);

        Self {
            _temp_dir: temp_dir,
            home,
            xdg_data,
            xdg_config,
            xdg_state,
        }
    }

    fn db_path(&self) -> PathBuf {
        self.xdg_data.join("aiobscura/data.db")
    }
}

fn seed_codex_fixture(home: &Path) {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../aiobscura-core/tests/fixtures/codex/minimal-session.jsonl");
    let target = home
        .join(".codex/sessions/2025/11/25")
        .join("rollout-2025-11-25T00-33-35-019ab86e-1e83-75b0-b2d7-d335492e7026.jsonl");

    fs::create_dir_all(target.parent().expect("missing fixture parent"))
        .expect("failed to create codex fixture directories");
    fs::copy(source, target).expect("failed to copy codex fixture");
}

fn run_bin(env: &CliTestEnv, bin_name: &str, args: &[&str]) -> Output {
    let bin_path = match bin_name {
        "aiobscura-sync" => PathBuf::from(assert_cmd::cargo::cargo_bin!("aiobscura-sync")),
        "aiobscura-analyze" => PathBuf::from(assert_cmd::cargo::cargo_bin!("aiobscura-analyze")),
        "aiobscura-collector" => {
            PathBuf::from(assert_cmd::cargo::cargo_bin!("aiobscura-collector"))
        }
        _ => panic!("unsupported binary in test harness: {bin_name}"),
    };

    let mut command = Command::new(bin_path);

    command
        .args(args)
        .env("HOME", &env.home)
        .env("XDG_DATA_HOME", &env.xdg_data)
        .env("XDG_CONFIG_HOME", &env.xdg_config)
        .env("XDG_STATE_HOME", &env.xdg_state)
        .output()
        .unwrap_or_else(|e| panic!("failed to execute {bin_name}: {e}"))
}

fn assert_success(bin_name: &str, args: &[&str], output: &Output) {
    if output.status.success() {
        return;
    }

    let rendered_args = args
        .iter()
        .map(|arg| OsString::from(arg).to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(" ");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    panic!(
        "{bin_name} {rendered_args} failed\nstatus: {}\nstdout:\n{}\nstderr:\n{}",
        output.status, stdout, stderr
    );
}

#[test]
fn sync_ingests_codex_fixture_and_populates_db() {
    let env = CliTestEnv::new();

    let output = run_bin(&env, "aiobscura-sync", &[]);
    assert_success("aiobscura-sync", &[], &output);

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Sync complete:"));
    assert!(
        stdout.contains("Messages inserted:"),
        "expected sync summary in stdout, got:\n{stdout}"
    );

    let db_path = env.db_path();
    assert!(
        db_path.exists(),
        "database file should exist at {}",
        db_path.display()
    );

    let db = Database::open(&db_path).expect("failed to open db");
    db.migrate().expect("failed to migrate db");

    let sessions = db
        .list_sessions(&SessionFilter::default())
        .expect("failed to list sessions");
    assert_eq!(sessions.len(), 1, "expected one synced session");
    assert_eq!(sessions[0].assistant, Assistant::Codex);

    let message_count = db
        .count_session_messages(&sessions[0].id)
        .expect("failed to count messages");
    assert!(
        message_count >= 5,
        "expected >=5 messages from fixture, got {}",
        message_count
    );
}

#[test]
fn analyze_and_collector_work_on_synced_database() {
    let env = CliTestEnv::new();

    let sync_output = run_bin(&env, "aiobscura-sync", &[]);
    assert_success("aiobscura-sync", &[], &sync_output);

    let list_plugins = run_bin(&env, "aiobscura-analyze", &["--list-plugins"]);
    assert_success("aiobscura-analyze", &["--list-plugins"], &list_plugins);
    let plugins_stdout = String::from_utf8_lossy(&list_plugins.stdout);
    assert!(plugins_stdout.contains("core.first_order"));
    assert!(plugins_stdout.contains("core.edit_churn"));

    let analyze_output = run_bin(&env, "aiobscura-analyze", &["--format", "text"]);
    assert_success("aiobscura-analyze", &["--format", "text"], &analyze_output);
    let analyze_stdout = String::from_utf8_lossy(&analyze_output.stdout);
    assert!(analyze_stdout.contains("Analyzing 1 session(s)"));

    let collector_status = run_bin(&env, "aiobscura-collector", &["status"]);
    assert_success("aiobscura-collector", &["status"], &collector_status);
    let collector_stdout = String::from_utf8_lossy(&collector_status.stdout);
    assert!(collector_stdout.contains("Catsyphon Collector Configuration"));
    assert!(collector_stdout.contains("Enabled:         false"));
}
