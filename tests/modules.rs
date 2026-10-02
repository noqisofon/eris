//! How the host hands `builtin` to scripts and `__native` to modules.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

struct Outcome {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

/// A scratch directory that can hold module files next to the script.
struct Dir(PathBuf);

impl Dir {
    fn new() -> Self {
        let d = std::env::temp_dir().join(format!(
            "eris-modules-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        Dir(d)
    }

    fn write(&self, name: &str, source: &str) -> PathBuf {
        let p = self.0.join(name);
        fs::write(&p, source).unwrap();
        p
    }

    fn run(&self, script: &str) -> Outcome {
        let path = self.write("main.eris", script);
        let o = Command::new(env!("CARGO_BIN_EXE_eris"))
            .arg("run")
            .arg(&path)
            .output()
            .unwrap();
        Outcome {
            code: o.status.code(),
            stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
        }
    }

    fn module(&self, name: &str) -> String {
        self.0.join(name).display().to_string()
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn script_that_is_not_a_closure_just_runs() {
    let d = Dir::new();
    let o = d.run("1 + 2");
    assert_eq!(o.code, Some(0), "stderr: {}", o.stderr);
}

#[test]
fn script_receives_the_builtin_module() {
    let d = Dir::new();
    let o = d.run(
        "{ builtin } -> let fmt = builtin.import \"fmt\"; in [ (fmt.println \"hello\") ]",
    );
    assert_eq!(o.code, Some(0), "stderr: {}", o.stderr);
    assert_eq!(o.stdout, "hello\n");
}

#[test]
fn script_with_extra_parameters_may_ignore_the_rest() {
    let d = Dir::new();
    assert_eq!(d.run("{ builtin; ... } -> 1").code, Some(0));
}

#[test]
fn a_parameter_the_host_does_not_provide_is_undefined_when_used() {
    let d = Dir::new();
    // Unused: fine. Used: a normal "not found" error.
    assert_eq!(d.run("{ foo } -> 1").code, Some(0));
    let o = d.run("{ foo } -> foo");
    assert_eq!(o.code, Some(1));
    assert!(o.stderr.contains("Variable 'foo' not found"), "stderr: {}", o.stderr);
}

#[test]
fn abort_exits_with_the_given_code() {
    let d = Dir::new();
    assert_eq!(d.run("{ builtin } -> builtin.abort 7").code, Some(7));
}

#[test]
fn imported_plain_module_is_returned_as_is() {
    let d = Dir::new();
    d.write("plain.eris", "{ a = 1; b = \"x\"; }");
    let o = d.run(&format!(
        "{{ builtin }} -> let fmt = builtin.import \"fmt\"; m = builtin.import \"{}\"; in [ (fmt.println m.a) (fmt.println m.b) ]",
        d.module("plain.eris")
    ));
    assert_eq!(o.code, Some(0), "stderr: {}", o.stderr);
    assert_eq!(o.stdout, "1\nx\n");
}

#[test]
fn imported_module_receives_the_native_table() {
    let d = Dir::new();
    d.write("greeter.eris", "{ __native } -> { say = |m| -> __native.fmt_println m; }");
    let o = d.run(&format!(
        "{{ builtin }} -> let m = builtin.import \"{}\"; in [ (m.say \"hi from a module\") ]",
        d.module("greeter.eris")
    ));
    assert_eq!(o.code, Some(0), "stderr: {}", o.stderr);
    assert_eq!(o.stdout, "hi from a module\n");
}

#[test]
fn imported_module_is_not_given_the_builtin_module() {
    // Only `__native` is injected into modules; `builtin` is for the main script.
    let d = Dir::new();
    d.write("wants.eris", "{ builtin } -> { x = builtin; }");
    let o = d.run(&format!(
        "{{ builtin }} -> let m = builtin.import \"{}\"; in m.x",
        d.module("wants.eris")
    ));
    assert_eq!(o.code, Some(1));
    assert!(o.stderr.contains("Variable 'builtin' not found"), "stderr: {}", o.stderr);
}

#[test]
fn every_bundled_module_can_be_imported() {
    let d = Dir::new();
    let imports: String = [
        "fmt", "list", "string", "attr", "fs", "json", "path", "child_process", "hash", "build",
    ]
    .iter()
    .map(|m| format!("{} = builtin.import \"{}\"; ", m, m))
    .collect();
    let o = d.run(&format!(
        "{{ builtin }} -> let {} in [ (fmt.println \"ok\") ]",
        imports
    ));
    assert_eq!(o.code, Some(0), "stderr: {}", o.stderr);
    assert_eq!(o.stdout, "ok\n");
}

#[test]
fn importing_a_missing_or_misspelled_module_is_an_error_with_a_hint() {
    let d = Dir::new();
    let o = d.run("{ builtin } -> builtin.import \"lst\"");
    assert_eq!(o.code, Some(1));
    assert!(o.stderr.contains("Did you mean 'list'?"), "stderr: {}", o.stderr);

    let o = d.run("{ builtin } -> builtin.import \"zzzzzzzzzz\"");
    assert_eq!(o.code, Some(1));
    assert!(o.stderr.contains("Module 'zzzzzzzzzz' not found"), "stderr: {}", o.stderr);
}

#[test]
fn importing_a_module_with_a_syntax_error_is_reported() {
    let d = Dir::new();
    d.write("bad.eris", "let x = ; in x");
    let o = d.run(&format!(
        "{{ builtin }} -> builtin.import \"{}\"",
        d.module("bad.eris")
    ));
    assert_eq!(o.code, Some(1));
    assert!(o.stderr.contains("Parse error in module"), "stderr: {}", o.stderr);
}

#[test]
fn import_requires_a_string() {
    let d = Dir::new();
    let o = d.run("{ builtin } -> builtin.import 5");
    assert_eq!(o.code, Some(1));
    assert!(o.stderr.contains("import expects a string"), "stderr: {}", o.stderr);
}
