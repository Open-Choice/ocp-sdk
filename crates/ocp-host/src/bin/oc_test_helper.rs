/// Minimal test-only binary that speaks enough of the ocp-json/1 protocol to
/// let `runner_tests.rs` exercise `run_tool_task` without a real plugin.
///
/// Invocation (matches what `run_tool_task` passes):
///   oc-test-helper run <task_file> --task <mode> --output-format protocol
///
/// The `--task` value doubles as the behaviour selector:
///   "ok"       — emits a `event.run.finished` envelope (success) and exits 0  (default)
///   "error"    — emits a `event.run.failed` envelope and exits 1
///   "bad_json" — writes a non-JSON line then exits 0
///   "hang"     — sleeps for 60 seconds (used to test timeout kill)
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args
        .windows(2)
        .find(|w| w[0] == "--task")
        .map(|w| w[1].as_str())
        .unwrap_or("ok")
        .to_string();

    match mode.as_str() {
        "hang" => {
            std::thread::sleep(std::time::Duration::from_secs(60));
        }
        "bad_json" => {
            println!("this is not valid json {{{{");
        }
        "error" => {
            println!(
                "{}",
                r#"{"ocp":"1","class":"event","id":{"fmt":"ulid","value":"01HQRZ8YV9XW6E2K8N9PJ4QF7M"},"ts":"2026-04-07T12:34:56.123456Z","kind":"event.run.failed","run":{"run_id":{"fmt":"ulid","value":"01HQRZ8YV9XW6E2K8N9PJ4QF8N"},"task_id":"error","tool":{"family":"oc-test-helper","name":"oc-test-helper","version":"0.0.0"}},"payload":{"reason":"requested error mode"}}"#
            );
            std::process::exit(1);
        }
        _ => {
            println!(
                "{}",
                r#"{"ocp":"1","class":"event","id":{"fmt":"ulid","value":"01HQRZ8YV9XW6E2K8N9PJ4QF7M"},"ts":"2026-04-07T12:34:56.123456Z","kind":"event.run.finished","run":{"run_id":{"fmt":"ulid","value":"01HQRZ8YV9XW6E2K8N9PJ4QF8N"},"task_id":"ok","tool":{"family":"oc-test-helper","name":"oc-test-helper","version":"0.0.0"}},"payload":{"ok":true,"elapsed":{"secs":0,"nanos":1000000},"artifacts":[]}}"#
            );
        }
    }
}
