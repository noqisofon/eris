//! End-to-end tests for `build.derivation`, run against the real binary.
#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// A scratch directory holding a script, a build input and a store.
struct Sandbox {
    dir: PathBuf,
}

impl Sandbox {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "eris-build-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Sandbox { dir }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    fn write(&self, name: &str, content: &str) -> PathBuf {
        let p = self.path(name);
        fs::write(&p, content).unwrap();
        p
    }

    /// Runs `drv_attrs` through `build.derivation` and returns
    /// `(hash, cached, out)` as printed by the script.
    fn build(&self, drv_attrs: &str, envs: &[(&str, &str)]) -> Result<(String, String, String), String> {
        let script = self.write(
            "main.eris",
            &format!(
                r#"{{ builtin }} ->
  let
    fmt = builtin.import "fmt";
    build = builtin.import "build";
    drv = build.derivation {{ {} }};
  in
    [ (fmt.println drv.hash) (fmt.println drv.cached) (fmt.println drv.out) ]
"#,
                drv_attrs
            ),
        );
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_eris"));
        cmd.arg("run")
            .arg(&script)
            .current_dir(&self.dir)
            .env("ERIS_STORE", "store"); // relative on purpose
        for (k, v) in envs {
            cmd.env(k, v);
        }
        let output = cmd.output().unwrap();
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).into_owned());
        }
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let mut lines = stdout.lines().map(str::to_string);
        Ok((
            lines.next().unwrap(),
            lines.next().unwrap(),
            lines.next().unwrap(),
        ))
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn attrs(name: &str, script: &Path) -> String {
    format!(
        r#"name = "{}"; builder = "/bin/sh"; args = [ p'{}' ];"#,
        name,
        script.display()
    )
}

#[test]
fn same_derivation_is_cached_on_second_build() {
    let sb = Sandbox::new();
    let script = sb.write("build.sh", "echo hi > \"$out\"\n");
    let a = attrs("t", &script);

    let (hash1, cached1, out1) = sb.build(&a, &[]).unwrap();
    let (hash2, cached2, out2) = sb.build(&a, &[]).unwrap();

    assert_eq!(cached1, "false");
    assert_eq!(cached2, "true");
    assert_eq!(hash1, hash2);
    assert_eq!(out1, out2);
    assert_eq!(fs::read_to_string(&out1).unwrap(), "hi\n");
}

#[test]
fn editing_a_build_script_changes_the_hash() {
    let sb = Sandbox::new();
    let script = sb.write("build.sh", "echo one > \"$out\"\n");
    let a = attrs("t", &script);
    let (hash1, _, out1) = sb.build(&a, &[]).unwrap();

    // Same attributes, same script path, different script contents.
    sb.write("build.sh", "echo two > \"$out\"\n");
    let (hash2, cached2, out2) = sb.build(&a, &[]).unwrap();

    assert_ne!(hash1, hash2);
    assert_eq!(cached2, "false");
    assert_eq!(fs::read_to_string(&out1).unwrap(), "one\n");
    assert_eq!(fs::read_to_string(&out2).unwrap(), "two\n");
}

#[test]
fn editing_the_builder_file_changes_the_hash() {
    let sb = Sandbox::new();
    let builder = sb.path("builder.sh");
    fs::write(&builder, "#!/bin/sh\necho one > \"$out\"\n").unwrap();
    fs::set_permissions(&builder, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let a = format!(r#"name = "t"; builder = p'{}';"#, builder.display());
    let (hash1, _, _) = sb.build(&a, &[]).unwrap();

    fs::write(&builder, "#!/bin/sh\necho two > \"$out\"\n").unwrap();
    let (hash2, cached2, out2) = sb.build(&a, &[]).unwrap();

    assert_ne!(hash1, hash2);
    assert_eq!(cached2, "false");
    assert_eq!(fs::read_to_string(&out2).unwrap(), "two\n");
}

#[test]
fn editing_a_file_inside_a_path_directory_changes_the_hash() {
    let sb = Sandbox::new();
    fs::create_dir_all(sb.path("src")).unwrap();
    sb.write("src/a.txt", "1");
    let script = sb.write("build.sh", "cat \"$1\"/a.txt > \"$out\"\n");
    let a = format!(
        r#"name = "t"; builder = "/bin/sh"; args = [ p'{}' p'{}' ];"#,
        script.display(),
        sb.path("src").display()
    );
    let (hash1, _, _) = sb.build(&a, &[]).unwrap();

    sb.write("src/a.txt", "2");
    let (hash2, _, out2) = sb.build(&a, &[]).unwrap();

    assert_ne!(hash1, hash2);
    assert_eq!(fs::read_to_string(&out2).unwrap(), "2");
}

#[test]
fn parent_environment_neither_reaches_the_builder_nor_affects_the_hash() {
    let sb = Sandbox::new();
    // Records what the builder can see of the evaluator's environment.
    let script = sb.write("build.sh", "echo \"foo=${FOO-unset} home=${HOME-unset}\" > \"$out\"\n");
    let a = attrs("t", &script);

    let (hash1, _, out1) = sb.build(&a, &[("FOO", "1"), ("HOME", "/tmp/a")]).unwrap();
    let (hash2, cached2, _) = sb.build(&a, &[("FOO", "2"), ("HOME", "/tmp/b")]).unwrap();

    assert_eq!(hash1, hash2);
    assert_eq!(cached2, "true");
    assert_eq!(fs::read_to_string(&out1).unwrap(), "foo=unset home=unset\n");
}

#[test]
fn declared_attributes_are_passed_to_the_builder_and_change_the_hash() {
    let sb = Sandbox::new();
    let script = sb.write("build.sh", "echo \"$GREETING\" > \"$out\"\n");
    let a1 = format!("{} GREETING = \"hello\";", attrs("t", &script));
    let a2 = format!("{} GREETING = \"bye\";", attrs("t", &script));

    let (hash1, _, out1) = sb.build(&a1, &[]).unwrap();
    let (hash2, _, out2) = sb.build(&a2, &[]).unwrap();

    assert_ne!(hash1, hash2);
    assert_eq!(fs::read_to_string(&out1).unwrap(), "hello\n");
    assert_eq!(fs::read_to_string(&out2).unwrap(), "bye\n");
}

#[test]
fn out_is_an_absolute_path_even_when_the_store_is_relative() {
    let sb = Sandbox::new();
    let script = sb.write(
        "build.sh",
        "case \"$out\" in /*) echo absolute > \"$out\" ;; *) echo relative > \"$out\" ;; esac\n",
    );
    let (_, _, out) = sb.build(&attrs("t", &script), &[]).unwrap();

    assert!(Path::new(&out).is_absolute(), "out was {}", out);
    assert_eq!(fs::read_to_string(&out).unwrap(), "absolute\n");
}

#[test]
fn out_attribute_is_rejected() {
    let sb = Sandbox::new();
    let script = sb.write("build.sh", "echo hi > \"$out\"\n");
    let err = sb
        .build(&format!("{} out = \"/tmp/evil\";", attrs("t", &script)), &[])
        .unwrap_err();

    assert!(err.contains("'out' is reserved"), "stderr was: {}", err);
}

#[test]
fn missing_builder_is_reported() {
    let sb = Sandbox::new();
    let err = sb
        .build(r#"name = "t"; builder = "no-such-builder-xyz";"#, &[])
        .unwrap_err();

    assert!(err.contains("not found in PATH"), "stderr was: {}", err);
}

#[test]
fn editing_the_target_of_a_symlinked_input_changes_the_hash() {
    let sb = Sandbox::new();
    sb.write("real.sh", "echo one > \"$out\"\n");
    std::os::unix::fs::symlink(sb.path("real.sh"), sb.path("link.sh")).unwrap();
    let a = attrs("t", &sb.path("link.sh"));
    let (hash1, _, _) = sb.build(&a, &[]).unwrap();

    sb.write("real.sh", "echo two > \"$out\"\n");
    let (hash2, cached2, out2) = sb.build(&a, &[]).unwrap();

    assert_ne!(hash1, hash2);
    assert_eq!(cached2, "false");
    assert_eq!(fs::read_to_string(&out2).unwrap(), "two\n");
}

#[test]
fn editing_the_target_of_a_symlinked_builder_changes_the_hash() {
    let sb = Sandbox::new();
    let real = sb.path("real-builder");
    fs::write(&real, "#!/bin/sh\necho b-one > \"$out\"\n").unwrap();
    fs::set_permissions(&real, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    std::os::unix::fs::symlink(&real, sb.path("mybuilder")).unwrap();
    let a = format!(r#"name = "t"; builder = p'{}';"#, sb.path("mybuilder").display());
    let (hash1, _, _) = sb.build(&a, &[]).unwrap();

    fs::write(&real, "#!/bin/sh\necho b-two > \"$out\"\n").unwrap();
    let (hash2, cached2, out2) = sb.build(&a, &[]).unwrap();

    assert_ne!(hash1, hash2);
    assert_eq!(cached2, "false");
    assert_eq!(fs::read_to_string(&out2).unwrap(), "b-two\n");
}

#[test]
fn builder_resolved_through_path_is_hashed_by_its_real_contents() {
    // `sh` is looked up on the builder's PATH; point that PATH at a directory
    // whose `sh` is a symlink to a script we then edit.
    let sb = Sandbox::new();
    fs::create_dir_all(sb.path("bin")).unwrap();
    let real = sb.path("real-sh");
    fs::write(&real, "#!/bin/sh\necho p-one > \"$out\"\n").unwrap();
    fs::set_permissions(&real, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    std::os::unix::fs::symlink(&real, sb.path("bin/mysh")).unwrap();
    let a = format!(
        r#"name = "t"; builder = "mysh"; PATH = "{}:/usr/bin:/bin";"#,
        sb.path("bin").display()
    );
    let (hash1, _, _) = sb.build(&a, &[]).unwrap();

    fs::write(&real, "#!/bin/sh\necho p-two > \"$out\"\n").unwrap();
    let (hash2, _, out2) = sb.build(&a, &[]).unwrap();

    assert_ne!(hash1, hash2);
    assert_eq!(fs::read_to_string(&out2).unwrap(), "p-two\n");
}

#[test]
fn editing_a_symlinked_file_inside_a_path_directory_changes_the_hash() {
    let sb = Sandbox::new();
    fs::create_dir_all(sb.path("src")).unwrap();
    sb.write("real.txt", "1");
    std::os::unix::fs::symlink(sb.path("real.txt"), sb.path("src/a.txt")).unwrap();
    let script = sb.write("build.sh", "cat \"$1\"/a.txt > \"$out\"\n");
    let a = format!(
        r#"name = "t"; builder = "/bin/sh"; args = [ p'{}' p'{}' ];"#,
        script.display(),
        sb.path("src").display()
    );
    let (hash1, _, _) = sb.build(&a, &[]).unwrap();

    sb.write("real.txt", "2");
    let (hash2, _, out2) = sb.build(&a, &[]).unwrap();

    assert_ne!(hash1, hash2);
    assert_eq!(fs::read_to_string(&out2).unwrap(), "2");
}

#[test]
fn broken_symlink_input_is_an_error() {
    let sb = Sandbox::new();
    std::os::unix::fs::symlink(sb.path("does-not-exist"), sb.path("dangling")).unwrap();
    let a = format!(
        r#"name = "t"; builder = "/bin/sh"; args = [ p'{}' ];"#,
        sb.path("dangling").display()
    );
    let err = sb.build(&a, &[]).unwrap_err();

    assert!(err.contains("broken symlink"), "stderr was: {}", err);
}

#[test]
fn symlink_cycle_in_an_input_directory_is_an_error() {
    let sb = Sandbox::new();
    fs::create_dir_all(sb.path("src")).unwrap();
    std::os::unix::fs::symlink(sb.path("src"), sb.path("src/loop")).unwrap();
    let script = sb.write("build.sh", "echo hi > \"$out\"\n");
    let a = format!(
        r#"name = "t"; builder = "/bin/sh"; args = [ p'{}' p'{}' ];"#,
        script.display(),
        sb.path("src").display()
    );
    let err = sb.build(&a, &[]).unwrap_err();

    assert!(err.contains("symlink cycle"), "stderr was: {}", err);
}

#[test]
fn making_an_input_script_executable_changes_the_hash() {
    let sb = Sandbox::new();
    let script = sb.write("build.sh", "echo hi > \"$out\"\n");
    let a = attrs("t", &script);
    let (hash1, _, _) = sb.build(&a, &[]).unwrap();

    fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let (hash2, _, _) = sb.build(&a, &[]).unwrap();

    assert_ne!(hash1, hash2);
}
