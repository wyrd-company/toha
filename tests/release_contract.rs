// ---
// relationships:
//   validates: architecture
//   references: cd
// ---
//! Execute the release workflow's artifact boundary without network or publication.
#![cfg(target_os = "linux")]

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, os::unix::fs::PermissionsExt, path::Path, process::Command};
use tempfile::TempDir;

const VERSION: &str = "1.2.3";
const COMMIT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn workflow() -> Value {
    serde_norway::from_str(include_str!("../.github/workflows/cd.yml")).unwrap()
}

fn steps<'a>(workflow: &'a Value, job: &str) -> &'a [Value] {
    workflow["jobs"][job]["steps"].as_array().unwrap()
}

fn named_step<'a>(workflow: &'a Value, job: &str, name: &str) -> &'a Value {
    steps(workflow, job)
        .iter()
        .find(|step| step["name"] == name)
        .unwrap_or_else(|| panic!("missing {job} step: {name}"))
}

fn executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn shell(root: &Path, script: &str, env: &[(&str, &str)]) -> Result<String, String> {
    let output = Command::new("bash")
        .args(["-euo", "pipefail", "-c", script])
        .current_dir(root)
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", root.join("bin").display()),
        )
        .env("GITHUB_REF_NAME", VERSION)
        .envs(env.iter().copied())
        .output()
        .unwrap();
    if output.status.success() {
        Ok(String::from_utf8(output.stdout).unwrap())
    } else {
        Err(format!(
            "workflow shell failed: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

fn fixture() -> TempDir {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path();
    fs::create_dir_all(root.join("bin")).unwrap();
    fs::create_dir_all(root.join("scripts/release")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        format!("[package]\nversion = \"{VERSION}\"\n"),
    )
    .unwrap();
    fs::write(root.join("LICENSE"), "fixture license\n").unwrap();
    fs::write(root.join("README.md"), "fixture readme\n").unwrap();
    fs::write(
        root.join("scripts/release/create-manifest.py"),
        include_str!("../scripts/release/create-manifest.py"),
    )
    .unwrap();
    fs::write(
        root.join("scripts/release/nfpm.yml"),
        include_str!("../scripts/release/nfpm.yml"),
    )
    .unwrap();
    executable(
        &root.join("bin/git"),
        &format!("#!/bin/bash\n[[ \"$*\" == 'rev-parse HEAD' ]]\nprintf '%s\\n' '{COMMIT}'\n"),
    );
    // Stub only the external package builder. The workflow stages and archives real files.
    executable(
        &root.join("bin/go"),
        "#!/bin/bash\nset -euo pipefail\ncase \"$*\" in\n  'env GOPATH') printf '%s/go\\n' \"$PWD\" ;;\n  'install github.com/goreleaser/nfpm/v2/cmd/nfpm@v2.47.0') ;;\n  *) exit 1 ;;\nesac\n",
    );
    fs::create_dir_all(root.join("go/bin")).unwrap();
    executable(
        &root.join("go/bin/nfpm"),
        "#!/bin/bash\nset -euo pipefail\n[[ $1 == package && $2 == --config && $3 == scripts/release/nfpm.yml && $4 == --packager && $6 == --target ]]\nprintf 'fixture %s %s %s\\n' \"$RELEASE_VERSION\" \"$PACKAGE_ARCH\" \"$5\" > \"$7\"\n",
    );
    fixture
}

fn archive_contract(workflow: &Value) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let mut artifacts = BTreeMap::new();
    let script = named_step(workflow, "build", "Assemble archive and Linux packages")["run"]
        .as_str()
        .unwrap();
    for arch in ["x86_64", "aarch64"] {
        let fixture = fixture();
        let root = fixture.path();
        let target = format!("{arch}-unknown-linux-gnu");
        let binary = root.join(format!("target/{target}/release/toha"));
        fs::create_dir_all(binary.parent().unwrap()).unwrap();
        executable(&binary, "#!/bin/bash\nprintf 'toha 1.2.3\\n'\n");
        shell(
            root,
            script,
            &[
                ("RELEASE_TARGET", &target),
                ("RELEASE_SYSTEM", "linux"),
                ("RELEASE_ARCH", arch),
            ],
        )?;
        let archive = format!("toha_{VERSION}_linux_{arch}.tar.gz");
        let output = shell(root, &format!("tar -tzf dist/{archive}"), &[])?;
        let mut members: Vec<_> = output
            .lines()
            .filter(|name| !matches!(*name, "." | "./"))
            .map(|name| name.strip_prefix("./").unwrap_or(name))
            .collect();
        members.sort_unstable();
        if members != ["LICENSE", "README.md", "toha"] {
            return Err(format!("Linux archive root members: {members:?}"));
        }
        for entry in fs::read_dir(root.join("dist")).unwrap() {
            let path = entry.unwrap().path();
            artifacts.insert(
                path.file_name().unwrap().to_str().unwrap().to_owned(),
                fs::read(path).unwrap(),
            );
        }
    }
    Ok(artifacts)
}

fn copy_artifacts(artifacts: &BTreeMap<String, Vec<u8>>, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for (name, bytes) in artifacts {
        fs::write(destination.join(name), bytes).unwrap();
    }
}

fn current_run_download(step: &Value) -> Result<(), String> {
    if step["with"].get("run-id").is_some() || step["with"].get("repository").is_some() {
        return Err("artifact download must use this workflow run".into());
    }
    Ok(())
}

fn manifest_contract(
    workflow: &Value,
    artifacts: &BTreeMap<String, Vec<u8>>,
) -> Result<(), String> {
    let fixture = fixture();
    let root = fixture.path();
    // A mutable Release deliberately contains different bytes with the same filenames.
    let mutable: BTreeMap<_, _> = artifacts
        .keys()
        .map(|name| (name.clone(), b"mutable release asset".to_vec()))
        .collect();
    copy_artifacts(&mutable, &root.join("mutable"));
    executable(
        &root.join("bin/gh"),
        "#!/bin/bash\nset -euo pipefail\n[[ $1 == release && $2 == download && $4 == --dir ]]\nmkdir -p \"$5\"\ncp mutable/* \"$5/\"\n",
    );
    let upload = steps(workflow, "build")
        .iter()
        .find(|step| {
            step["uses"]
                .as_str()
                .is_some_and(|value| value.starts_with("actions/upload-artifact@"))
        })
        .unwrap();
    let build_name = upload["with"]["name"].as_str().unwrap();
    if build_name != "toha-${{ matrix.system }}-${{ matrix.arch }}"
        || upload["with"]["path"] != "dist/*"
    {
        return Err("build artifact fixture does not match the workflow upload".into());
    }
    let mut retained = None;
    let mut downloaded_build = false;
    for step in steps(workflow, "release") {
        let action = step["uses"].as_str().unwrap_or("");
        if action.starts_with("actions/download-artifact@") {
            current_run_download(step)?;
            if step["with"]["pattern"] != "toha-*" || step["with"]["merge-multiple"] != true {
                return Err("manifest source must download this run's build artifacts".into());
            }
            let path = step["with"]["path"].as_str().unwrap();
            copy_artifacts(artifacts, &root.join(path));
            downloaded_build = true;
        } else if action.starts_with("actions/upload-artifact@") {
            if !downloaded_build || step["with"]["name"] != "package-release-manifest" {
                return Err("manifest must be retained after downloading build artifacts".into());
            }
            let path = step["with"]["path"].as_str().unwrap();
            retained = Some(fs::read(root.join(path)).map_err(|err| err.to_string())?);
            break; // Publication starts after the retained handoff.
        } else if let Some(script) = step["run"].as_str() {
            shell(root, script, &[])?;
        } else if !action.starts_with("actions/checkout@") {
            return Err(format!("unsupported release artifact producer: {action}"));
        }
    }
    let retained = retained.ok_or("missing retained package manifest")?;
    let manifest: Value = serde_json::from_slice(&retained).unwrap();
    if manifest["source"]["commit"] != COMMIT || manifest["version"] != VERSION {
        return Err("manifest release identity differs from this build".into());
    }
    let entries = manifest["artifacts"].as_array().unwrap();
    if entries.len() != artifacts.len() {
        return Err("manifest must describe all six Linux build artifacts".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    for entry in entries {
        let name = entry["filename"].as_str().unwrap();
        let bytes = artifacts
            .get(name)
            .ok_or("manifest names an artifact outside this build")?;
        if !seen.insert(name) || entry["sha256"] != format!("{:x}", Sha256::digest(bytes)) {
            return Err(format!(
                "manifest digest differs from own build artifact: {name}"
            ));
        }
    }
    let mut handoff = None;
    let mut submitted = false;
    for step in steps(workflow, "packages") {
        let action = step["uses"].as_str().unwrap_or("");
        if action.starts_with("actions/download-artifact@") {
            current_run_download(step)?;
            if step["with"]["name"] != "package-release-manifest" {
                return Err("package submission must consume the retained manifest".into());
            }
            let directory = root.join(step["with"]["path"].as_str().unwrap());
            fs::create_dir_all(&directory).unwrap();
            let path = directory.join("release-manifest.json");
            fs::write(&path, &retained).unwrap();
            handoff = Some(path);
        } else if action.starts_with("wyrd-company/repo.wyrd.foo/.github/actions/submit-release@") {
            let path = root.join(step["with"]["manifest"].as_str().unwrap());
            if handoff.as_ref() != Some(&path) || fs::read(path).unwrap() != retained {
                return Err("submitted manifest differs from retained build handoff".into());
            }
            submitted = true;
        } else if !action.starts_with("actions/checkout@") {
            return Err("package submission has another mutable input".into());
        }
    }
    if !submitted {
        return Err("missing retained-manifest package submission".into());
    }
    Ok(())
}

#[test]
fn linux_archives_and_package_manifest_come_from_this_build() {
    let workflow = workflow();
    let artifacts = archive_contract(&workflow).unwrap();
    manifest_contract(&workflow, &artifacts).unwrap();
}

#[test]
fn archive_contract_rejects_missing_extra_and_nested_members() {
    for replacement in [
        "cp LICENSE stage/",
        "cp LICENSE README.md Cargo.toml stage/",
        "mkdir stage/docs; cp LICENSE stage/; cp README.md stage/docs/",
    ] {
        let mut workflow = workflow();
        let step = workflow["jobs"]["build"]["steps"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|step| step["name"] == "Assemble archive and Linux packages")
            .unwrap();
        let script = step["run"]
            .as_str()
            .unwrap()
            .replace("cp LICENSE README.md stage/", replacement);
        step["run"] = script.into();
        let error = archive_contract(&workflow).unwrap_err();
        assert!(error.contains("Linux archive root members"), "{error}");
    }
}

#[test]
fn manifest_contract_rejects_mutable_release_downloads() {
    let mut workflow = workflow();
    let artifacts = archive_contract(&workflow).unwrap();
    let step = workflow["jobs"]["release"]["steps"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|step| step["name"] == "Create checksums and package handoff from build artifacts")
        .unwrap();
    let script = step["run"].as_str().unwrap().replace(
        "python3 scripts/release/create-manifest.py dist",
        "gh release download \"$GITHUB_REF_NAME\" --dir downloaded\npython3 scripts/release/create-manifest.py downloaded",
    );
    step["run"] = script.into();
    let error = manifest_contract(&workflow, &artifacts).unwrap_err();
    assert!(
        error.contains("manifest digest differs from own build artifact"),
        "{error}"
    );
}

#[test]
fn manifest_contract_rejects_artifacts_from_another_run() {
    let workflow = workflow();
    let artifacts = archive_contract(&workflow).unwrap();
    for job in ["release", "packages"] {
        let mut workflow = workflow.clone();
        let download = workflow["jobs"][job]["steps"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|step| {
                step["uses"]
                    .as_str()
                    .is_some_and(|action| action.starts_with("actions/download-artifact@"))
            })
            .unwrap();
        download["with"]["run-id"] = "12345".into();
        let error = manifest_contract(&workflow, &artifacts).unwrap_err();
        assert!(
            error.contains("artifact download must use this workflow run"),
            "{error}"
        );
    }
}

const ELF_GATE: &str = include_str!("../scripts/release/verify-linux-elf.py");

fn elf_gate(workflow: &Value, script: &str, inspection: &str) -> Result<(), String> {
    let step = named_step(workflow, "build", "Verify Linux ELF compatibility");
    if step["if"] != "matrix.system == 'linux'" {
        return Err("ELF gate must run for both Linux targets".into());
    }
    for target in workflow["jobs"]["build"]["strategy"]["matrix"]["include"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|target| target["system"] == "linux")
    {
        let fixture = fixture();
        let root = fixture.path();
        fs::write(root.join("scripts/release/verify-linux-elf.py"), script).unwrap();
        fs::write(root.join("inspection"), inspection).unwrap();
        executable(
            &root.join("bin/readelf"),
            "#!/bin/bash\nset -euo pipefail\n[[ \"$*\" == \"--wide --version-info --dynamic target/$RELEASE_TARGET/release/toha\" ]]\ncat inspection\n",
        );
        let target = target["target"].as_str().unwrap();
        let declared_target = step["env"]["RELEASE_TARGET"]
            .as_str()
            .unwrap()
            .replace("${{ matrix.target }}", target);
        let output = shell(
            root,
            step["run"].as_str().unwrap(),
            &[("RELEASE_TARGET", &declared_target)],
        )?;
        if !output.contains(&format!("target/{target}/release/toha: GLIBC <= 2.17")) {
            return Err(format!("ELF gate did not verify {target}"));
        }
    }
    Ok(())
}

const GOOD_ELF: &str = "  (NEEDED) Shared library: [libc.so.6]\n  (NEEDED) Shared library: [libm.so.6]\n  Name: GLIBC_2.2.5\n  Name: GLIBC_2.17\n";

#[test]
fn elf_gate_checks_both_linux_targets_and_rejects_host_dependencies() {
    let workflow = workflow();
    elf_gate(&workflow, ELF_GATE, GOOD_ELF).unwrap();
    let error = elf_gate(
        &workflow,
        ELF_GATE,
        &GOOD_ELF.replace("GLIBC_2.17", "GLIBC_2.18"),
    )
    .unwrap_err();
    assert!(error.contains("GLIBC floor exceeds 2.17"), "{error}");
    for dependency in ["libcurl.so.4", "libz.so.1", "libssl.so.3", "libcrypto.so.3"] {
        let bad = format!("{GOOD_ELF}  (NEEDED) Shared library: [{dependency}]\n");
        let error = elf_gate(&workflow, ELF_GATE, &bad).unwrap_err();
        assert!(
            error.contains("Unexpected dynamic dependencies") && error.contains(dependency),
            "{error}"
        );
    }
    for (bad, assertion) in [
        (
            "  (NEEDED) Shared library: [libc.so.6]\n".to_owned(),
            "No GLIBC version requirements found",
        ),
        (
            GOOD_ELF.replace("GLIBC_2.17", "GLIBC_ABI_DT_RELR"),
            "GLIBC floor exceeds 2.17",
        ),
        (
            GOOD_ELF.replace("  (NEEDED) Shared library: [libc.so.6]\n", ""),
            "Missing libc dynamic dependency",
        ),
    ] {
        let error = elf_gate(&workflow, ELF_GATE, &bad).unwrap_err();
        assert!(error.contains(assertion), "{error}");
    }
}

#[test]
fn elf_gate_mutations_expose_each_compatibility_guard() {
    let workflow = workflow();
    for (guard, bad) in [
        (
            "if not versions:",
            "  (NEEDED) Shared library: [libc.so.6]\n".to_owned(),
        ),
        (
            "if (\n        not re.fullmatch(r\"[0-9]+(?:\\.[0-9]+)+\", version)\n        or tuple(map(int, version.split(\".\"))) > (2, 17)\n    ):",
            GOOD_ELF.replace("GLIBC_2.17", "GLIBC_2.18"),
        ),
        (
            "if \"libc.so.6\" not in needed:",
            GOOD_ELF.replace("  (NEEDED) Shared library: [libc.so.6]\n", ""),
        ),
        (
            "if unexpected:",
            format!("{GOOD_ELF}  (NEEDED) Shared library: [libcurl.so.4]\n"),
        ),
    ] {
        assert!(ELF_GATE.contains(guard), "missing mutation site: {guard}");
        let mutated = ELF_GATE.replace(guard, "if False:");
        elf_gate(&workflow, &mutated, &bad).unwrap();
    }
    let mut narrowed = workflow.clone();
    let step = narrowed["jobs"]["build"]["steps"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|step| step["name"] == "Verify Linux ELF compatibility")
        .unwrap();
    step["if"] = "matrix.target == 'x86_64-unknown-linux-gnu'".into();
    assert!(
        elf_gate(&narrowed, ELF_GATE, GOOD_ELF)
            .unwrap_err()
            .contains("both Linux targets")
    );
}

fn zig_install_contract(workflow: &Value) -> Result<(), String> {
    let action = named_step(workflow, "build", "Install cargo-zigbuild")["uses"]
        .as_str()
        .unwrap();
    let revision = action.strip_prefix("taiki-e/install-action@").unwrap();
    if revision.len() != 40 || !revision.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("cargo-zigbuild installer must use an immutable action revision".into());
    }
    let fixture = fixture();
    let root = fixture.path();
    fs::write(
        root.join("scripts/release/zig-requirements.txt"),
        include_str!("../scripts/release/zig-requirements.txt"),
    )
    .unwrap();
    executable(
        &root.join("bin/python3"),
        "#!/bin/bash\nset -euo pipefail\n[[ $1 == -m && $2 == venv ]]\nmkdir -p \"$3/bin\"\nprintf '%s\\n' \"$*\" > venv-invocation\ncp venv-python \"$3/bin/python\"\n",
    );
    executable(
        &root.join("venv-python"),
        "#!/bin/bash\nset -euo pipefail\nif [[ $1 == -m && $2 == pip && $3 == install ]]; then\n  printf '%s\\n' \"$0\" \"$*\" > pip-invocation\nelif [[ $1 == -c ]]; then\n  printf '%s/ziglang\\n' \"$PWD\"\nelse\n  exit 1\nfi\n",
    );
    fs::create_dir(root.join("ziglang")).unwrap();
    executable(
        &root.join("ziglang/zig"),
        "#!/bin/bash\necho 'fixture Zig'\n",
    );
    let runner_temp = root.join("runner-temp");
    let github_path = root.join("github-path");
    shell(
        root,
        named_step(workflow, "build", "Install Zig")["run"]
            .as_str()
            .unwrap(),
        &[
            ("RUNNER_TEMP", runner_temp.to_str().unwrap()),
            ("GITHUB_PATH", github_path.to_str().unwrap()),
        ],
    )?;
    let invocation =
        fs::read_to_string(root.join("pip-invocation")).map_err(|err| err.to_string())?;
    if !invocation.contains("/runner-temp/toha-zig/bin/python\n") {
        return Err("Zig must install in an isolated venv".into());
    }
    let arguments: Vec<_> = invocation
        .lines()
        .nth(1)
        .unwrap()
        .split_whitespace()
        .collect();
    if !arguments.contains(&"--require-hashes")
        || !arguments
            .windows(2)
            .any(|pair| pair == ["-r", "scripts/release/zig-requirements.txt"])
    {
        return Err("Zig must install with hash-locked requirements".into());
    }
    let path = fs::read_to_string(github_path).unwrap();
    if !Path::new(path.trim()).join("zig").is_file() {
        return Err("Zig executable directory must reach later workflow steps".into());
    }
    Ok(())
}

#[test]
fn zig_install_uses_immutable_tools_and_an_isolated_hash_check() {
    zig_install_contract(&workflow()).unwrap();
}

#[test]
fn zig_install_mutations_expose_mutable_action_and_unchecked_wheel() {
    let workflow = workflow();
    for (name, mutation, expected) in [
        (
            "Install cargo-zigbuild",
            "mutable-action",
            "immutable action revision",
        ),
        ("Install Zig", "unchecked-wheel", "hash-locked requirements"),
        ("Install Zig", "global-pip", "workflow shell failed"),
        ("Install Zig", "missing-path", "Zig executable directory"),
    ] {
        let mut workflow = workflow.clone();
        let step = workflow["jobs"]["build"]["steps"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|step| step["name"] == name)
            .unwrap();
        if mutation == "mutable-action" {
            step["uses"] = "taiki-e/install-action@v2".into();
        } else {
            let script = step["run"].as_str().unwrap();
            step["run"] = match mutation {
                "unchecked-wheel" => script.replace("--require-hashes", ""),
                "global-pip" => script.replace("\"$zig_venv/bin/python\" -m pip", "python3 -m pip"),
                "missing-path" => {
                    script.replace(">> \"$GITHUB_PATH\"", "> /dev/null; touch \"$GITHUB_PATH\"")
                }
                _ => unreachable!(),
            }
            .into();
        }
        let error = zig_install_contract(&workflow).unwrap_err();
        assert!(error.contains(expected), "{mutation}: {error}");
    }
}

fn transport_commands_contract(ci: &Value, taskfile: &Value) -> Result<(), String> {
    for (runner, task_platform) in [
        ("Linux", "linux"),
        ("macOS", "darwin"),
        ("Windows", "windows"),
    ] {
        for driver in ["CI", "Task"] {
            let fixture = fixture();
            let root = fixture.path();
            executable(
                &root.join("bin/cargo"),
                "#!/bin/bash\nprintf '%s\\n' \"$*\" >> cargo-invocations\n",
            );
            let mut output = String::new();
            if driver == "CI" {
                let job = if runner == "Linux" {
                    "test"
                } else {
                    "platforms"
                };
                for step in steps(ci, job).iter().filter(|step| {
                    step["name"]
                        .as_str()
                        .is_some_and(|name| name.contains("Git transport"))
                }) {
                    let condition = step["if"].as_str().unwrap_or("");
                    let enabled = match condition {
                        "" => true,
                        "runner.os != 'Windows'" => runner != "Windows",
                        "runner.os == 'Windows'" => runner == "Windows",
                        _ => {
                            return Err(format!("unsupported CI transport condition: {condition}"));
                        }
                    };
                    if enabled {
                        output.push_str(&shell(root, step["run"].as_str().unwrap(), &[])?);
                    }
                }
            } else {
                for command in taskfile["tasks"]["test:network"]["cmds"]
                    .as_array()
                    .unwrap()
                {
                    let (script, enabled) = if let Some(script) = command.as_str() {
                        (script, true)
                    } else {
                        let enabled = command["platforms"].as_array().is_none_or(|platforms| {
                            platforms.iter().any(|platform| platform == task_platform)
                        });
                        (command["cmd"].as_str().unwrap(), enabled)
                    };
                    if enabled {
                        output.push_str(&shell(root, script, &[])?);
                    }
                }
            }
            let invocations =
                fs::read_to_string(root.join("cargo-invocations")).unwrap_or_default();
            let https = invocations
                .lines()
                .filter(|line| line.contains("--test source_https "))
                .count();
            let ssh = invocations
                .lines()
                .filter(|line| line.contains("--test source_ssh "))
                .count();
            if https != 1 || ssh != usize::from(runner != "Windows") {
                return Err(format!(
                    "{driver} {runner} must run HTTPS and only Unix SSH checks: {invocations}"
                ));
            }
            if runner == "Windows" && !output.contains("Windows SSH coverage is skipped") {
                return Err(format!(
                    "{driver} must report Windows SSH coverage is skipped"
                ));
            }
        }
    }
    Ok(())
}

#[test]
fn transport_commands_report_unix_ssh_coverage_and_windows_skip() {
    let ci = serde_norway::from_str(include_str!("../.github/workflows/ci.yml")).unwrap();
    let taskfile = serde_norway::from_str(include_str!("../Taskfile.yml")).unwrap();
    transport_commands_contract(&ci, &taskfile).unwrap();
}

#[test]
fn transport_command_mutations_expose_windows_ssh_false_coverage() {
    let ci: Value = serde_norway::from_str(include_str!("../.github/workflows/ci.yml")).unwrap();
    let taskfile: Value = serde_norway::from_str(include_str!("../Taskfile.yml")).unwrap();
    let mut all_platforms = ci.clone();
    for step in all_platforms["jobs"]["platforms"]["steps"]
        .as_array_mut()
        .unwrap()
    {
        if step["name"] == "Local Unix SSH Git transport check" {
            step.as_object_mut().unwrap().remove("if");
        }
    }
    assert!(
        transport_commands_contract(&all_platforms, &taskfile)
            .unwrap_err()
            .contains("CI Windows")
    );
    let mut no_skip = ci.clone();
    no_skip["jobs"]["platforms"]["steps"]
        .as_array_mut()
        .unwrap()
        .retain(|step| step["name"] != "SSH Git transport coverage is unavailable on Windows");
    assert!(
        transport_commands_contract(&no_skip, &taskfile)
            .unwrap_err()
            .contains("CI must report Windows SSH")
    );
    let mut all_platforms = taskfile.clone();
    for command in all_platforms["tasks"]["test:network"]["cmds"]
        .as_array_mut()
        .unwrap()
    {
        if command["cmd"]
            .as_str()
            .is_some_and(|command| command.contains("source_ssh"))
        {
            command.as_object_mut().unwrap().remove("platforms");
        }
    }
    assert!(
        transport_commands_contract(&ci, &all_platforms)
            .unwrap_err()
            .contains("Task Windows")
    );
    let mut no_skip = taskfile.clone();
    no_skip["tasks"]["test:network"]["cmds"]
        .as_array_mut()
        .unwrap()
        .retain(|command| {
            !command["cmd"]
                .as_str()
                .is_some_and(|command| command.contains("Windows SSH coverage"))
        });
    assert!(
        transport_commands_contract(&ci, &no_skip)
            .unwrap_err()
            .contains("Task must report Windows SSH")
    );
}

// Crate publication: the job selector, Trusted Publishing authentication and
// the 0.4.0 recovery source identity. The job scripts run against a real Git
// fixture with Cargo and the crates.io index stubbed.

const AUTH_ACTION: &str =
    "rust-lang/crates-io-auth-action@3268eb413133e2960fa4ae0e963ca76380c54cd9";
const AUTH_TOKEN: &str = "${{ steps.auth.outputs.token }}";
const RECOVERY_MODE: &str = "crates-0.4.0-recovery";
const RECOVERY_VERSION: &str = "0.4.0";
const RECOVERY_COMMIT: &str = "e85881a5cab643488fe0000b982177d18784fad1";
const RECOVERY_TAG_OBJECT: &str = "eebc73aa821c9e764f7ec29be8667c7810cb79e7";
const PUBLISHING_JOBS: [&str; 2] = ["crates", "crates-recovery"];

#[derive(Debug, PartialEq)]
enum Operand {
    Text(String),
    Null,
    Bool(bool),
}

impl Operand {
    fn truthy(&self) -> bool {
        match self {
            Operand::Text(text) => !text.is_empty(),
            Operand::Null => false,
            Operand::Bool(value) => *value,
        }
    }

    fn equals(&self, other: &Operand) -> bool {
        match (self, other) {
            (Operand::Text(left), Operand::Text(right)) => left.eq_ignore_ascii_case(right),
            (left, right) => left == right,
        }
    }
}

/// A GitHub expression over the operands CD job conditions use:
/// `github.event_name`, `inputs.mode` and string literals, joined by `==`,
/// `!=`, `&&`, `||` and parentheses. Anything else, such as a status function,
/// is an error.
struct Condition<'a> {
    tokens: Vec<String>,
    at: usize,
    event: &'a str,
    mode: Option<&'a str>,
}

impl Condition<'_> {
    fn evaluate(text: &str, event: &str, mode: Option<&str>) -> Result<bool, String> {
        let mut tokens = Vec::new();
        let mut rest = text.trim();
        while !rest.is_empty() {
            let token = if let Some(literal) = rest.strip_prefix('\'') {
                let end = literal.find('\'').ok_or("unterminated literal")?;
                rest[..end + 2].to_owned()
            } else if let Some(operator) = ["==", "!=", "&&", "||", "(", ")"]
                .into_iter()
                .find(|operator| rest.starts_with(operator))
            {
                operator.to_owned()
            } else {
                let end = rest
                    .find(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '_'))
                    .unwrap_or(rest.len());
                if end == 0 {
                    return Err(format!("unsupported condition: {text}"));
                }
                rest[..end].to_owned()
            };
            rest = rest[token.len()..].trim_start();
            tokens.push(token);
        }
        let mut condition = Condition {
            tokens,
            at: 0,
            event,
            mode,
        };
        let value = condition.or()?;
        if condition.at != condition.tokens.len() {
            return Err(format!("unsupported condition: {text}"));
        }
        Ok(value.truthy())
    }

    fn next_is(&self, token: &str) -> bool {
        self.tokens.get(self.at).is_some_and(|next| next == token)
    }

    fn or(&mut self) -> Result<Operand, String> {
        let mut value = self.and()?;
        while self.next_is("||") {
            self.at += 1;
            let right = self.and()?;
            value = Operand::Bool(value.truthy() || right.truthy());
        }
        Ok(value)
    }

    fn and(&mut self) -> Result<Operand, String> {
        let mut value = self.compare()?;
        while self.next_is("&&") {
            self.at += 1;
            let right = self.compare()?;
            value = Operand::Bool(value.truthy() && right.truthy());
        }
        Ok(value)
    }

    fn compare(&mut self) -> Result<Operand, String> {
        let left = self.primary()?;
        for (operator, equal) in [("==", true), ("!=", false)] {
            if self.next_is(operator) {
                self.at += 1;
                let right = self.primary()?;
                return Ok(Operand::Bool(left.equals(&right) == equal));
            }
        }
        Ok(left)
    }

    fn primary(&mut self) -> Result<Operand, String> {
        let token = self
            .tokens
            .get(self.at)
            .ok_or("incomplete condition")?
            .clone();
        self.at += 1;
        match token.as_str() {
            "(" => {
                let value = self.or()?;
                if !self.next_is(")") {
                    return Err("unbalanced condition".into());
                }
                self.at += 1;
                Ok(value)
            }
            "github.event_name" => Ok(Operand::Text(self.event.to_owned())),
            "inputs.mode" => Ok(self
                .mode
                .map_or(Operand::Null, |mode| Operand::Text(mode.to_owned()))),
            literal if literal.starts_with('\'') => {
                Ok(Operand::Text(literal.trim_matches('\'').to_owned()))
            }
            other => Err(format!("unsupported condition operand: {other}")),
        }
    }
}

/// The jobs a run for `event` starts. A job runs when its condition holds and
/// every job it needs ran; a status function in a condition is unsupported.
fn planned_jobs(
    workflow: &Value,
    event: &str,
    mode: Option<&str>,
) -> Result<std::collections::BTreeSet<String>, String> {
    let jobs = workflow["jobs"].as_object().unwrap();
    let mut planned = std::collections::BTreeSet::new();
    let mut decided = std::collections::BTreeSet::new();
    while decided.len() < jobs.len() {
        let before = decided.len();
        for (name, job) in jobs {
            let needs: Vec<&str> = match &job["needs"] {
                Value::Null => vec![],
                Value::String(need) => vec![need.as_str()],
                Value::Array(needs) => needs.iter().filter_map(Value::as_str).collect(),
                other => return Err(format!("unsupported needs: {other}")),
            };
            if decided.contains(name) || !needs.iter().all(|need| decided.contains(*need)) {
                continue;
            }
            let condition = match &job["if"] {
                Value::Null => true,
                Value::String(text) => Condition::evaluate(text, event, mode)?,
                other => return Err(format!("unsupported job condition: {other}")),
            };
            if condition && needs.iter().all(|need| planned.contains(*need)) {
                planned.insert(name.clone());
            }
            decided.insert(name.clone());
        }
        if decided.len() == before {
            return Err("job dependencies do not resolve".into());
        }
    }
    Ok(planned)
}

fn job_set(names: &[&str]) -> std::collections::BTreeSet<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

fn selector_contract(workflow: &Value) -> Result<(), String> {
    let input = &workflow["on"]["workflow_dispatch"]["inputs"]["mode"];
    if input["type"] != "choice"
        || input["options"] != serde_json::json!(["build-only", RECOVERY_MODE])
        || input["default"] != "build-only"
    {
        return Err("dispatch mode must default to build-only with one recovery choice".into());
    }
    let push = planned_jobs(workflow, "push", None)?;
    let channels = [
        "credential-preflight",
        "build",
        "release",
        "crates",
        "homebrew",
        "packages",
    ];
    if push != job_set(&channels) {
        return Err(format!(
            "a tag push must run every release channel: {push:?}"
        ));
    }
    // The default, the explicit choice, and an omitted input all build only.
    for mode in [input["default"].as_str(), Some("build-only"), None] {
        let manual = planned_jobs(workflow, "workflow_dispatch", mode)?;
        if manual != job_set(&["credential-preflight", "build"]) {
            return Err(format!("a build-only dispatch must only build: {manual:?}"));
        }
    }
    let recovery = planned_jobs(workflow, "workflow_dispatch", Some(RECOVERY_MODE))?;
    if recovery != job_set(&["crates-recovery"]) {
        return Err(format!(
            "a recovery dispatch must run only crates-recovery: {recovery:?}"
        ));
    }
    if !workflow["jobs"]["crates-recovery"]["needs"].is_null() {
        return Err("crates-recovery must not depend on another job".into());
    }
    Ok(())
}

fn strings<'a>(value: &'a Value, found: &mut Vec<&'a str>) {
    match value {
        Value::String(text) => found.push(text),
        Value::Array(items) => items.iter().for_each(|item| strings(item, found)),
        Value::Object(map) => map.values().for_each(|item| strings(item, found)),
        _ => {}
    }
}

fn contains(value: &Value, needle: &str) -> bool {
    let mut found = Vec::new();
    strings(value, &mut found);
    found.iter().any(|text| text.contains(needle))
}

fn auth_contract(workflow: &Value) -> Result<(), String> {
    if workflow["permissions"] != serde_json::json!({"contents": "read"}) {
        return Err("workflow permissions must stay read-only".into());
    }
    if contains(workflow, "secrets.CARGO_REGISTRY_TOKEN") || contains(workflow, "cargo login") {
        return Err("crate publication must not use a stored crates.io token".into());
    }
    for (name, job) in workflow["jobs"].as_object().unwrap() {
        let publishing = PUBLISHING_JOBS.contains(&name.as_str());
        if publishing != (job["permissions"]["id-token"] == "write") {
            return Err(format!(
                "only crate publishing jobs may request id-token: {name}"
            ));
        }
        if job.get("environment").is_some() {
            return Err(format!("{name} must not name an environment"));
        }
    }
    for name in PUBLISHING_JOBS {
        let job = &workflow["jobs"][name];
        if job["permissions"] != serde_json::json!({"contents": "read", "id-token": "write"}) {
            return Err(format!(
                "{name} permissions must be contents read and id-token write"
            ));
        }
        if contains(job, "secrets.") {
            return Err(format!("{name} must not read a repository secret"));
        }
        let steps = steps(workflow, name);
        let auth: Vec<_> = steps
            .iter()
            .enumerate()
            .filter(|(_, step)| contains(&step["uses"], "crates-io-auth-action"))
            .collect();
        let [(at, step)] = auth.as_slice() else {
            return Err(format!("{name} must authenticate exactly once"));
        };
        if step["uses"] != AUTH_ACTION
            || step["id"] != "auth"
            || step.get("with").is_some()
            || step.get("if").is_some()
            || step.get("continue-on-error").is_some()
        {
            return Err(format!(
                "{name} must use the pinned official auth action as id auth"
            ));
        }
        // Authentication sits right before publication, after packaging.
        let publish = steps.get(at + 1).ok_or("missing publication after auth")?;
        if publish["name"] != "Publish or verify the crate"
            || publish["env"] != serde_json::json!({"CARGO_REGISTRY_TOKEN": AUTH_TOKEN})
            || publish.get("if").is_some()
            || publish.get("continue-on-error").is_some()
        {
            return Err(format!(
                "{name} must publish next with only the auth output token"
            ));
        }
        for (index, step) in steps.iter().enumerate() {
            if index != at + 1
                && (contains(step, "steps.auth.outputs") || contains(step, "ensure-crate.sh"))
            {
                return Err(format!(
                    "{name} uses the temporary token or publishes in another step"
                ));
            }
        }
    }
    Ok(())
}

fn recovery_identity_contract(workflow: &Value) -> Result<(), String> {
    let checkout = &steps(workflow, "crates-recovery")[0];
    if !contains(&checkout["uses"], "actions/checkout@")
        || checkout["with"]["ref"] != RECOVERY_COMMIT
        || checkout["with"]["fetch-depth"] != 0
        || checkout["with"]["persist-credentials"] != false
    {
        return Err("recovery must check out full history at the 0.4.0 commit".into());
    }
    let identity = named_step(
        workflow,
        "crates-recovery",
        "Verify recovery source identity",
    );
    let package = named_step(
        workflow,
        "crates-recovery",
        "Package the crate before authentication",
    );
    if identity["env"]
        != serde_json::json!({
            "RECOVERY_VERSION": RECOVERY_VERSION,
            "RECOVERY_COMMIT": RECOVERY_COMMIT,
            "RECOVERY_TAG_OBJECT": RECOVERY_TAG_OBJECT,
        })
        || package["env"] != serde_json::json!({"RECOVERY_COMMIT": RECOVERY_COMMIT})
    {
        return Err("recovery identity must name the immutable 0.4.0 tag and commit".into());
    }
    let publish = named_step(workflow, "crates-recovery", "Publish or verify the crate");
    let invocations: Vec<Vec<&str>> = publish["run"]
        .as_str()
        .unwrap()
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>())
        .filter(|words| words.first() == Some(&"scripts/release/ensure-crate.sh"))
        .collect();
    if invocations != [["scripts/release/ensure-crate.sh", RECOVERY_VERSION]] {
        return Err("recovery must run the tagged ensure-crate.sh with literal 0.4.0".into());
    }
    Ok(())
}

#[test]
fn crate_publication_runs_only_where_its_mode_selects() {
    selector_contract(&workflow()).unwrap();
}

#[test]
fn crate_publication_authenticates_only_through_trusted_publishing() {
    let workflow = workflow();
    auth_contract(&workflow).unwrap();
    recovery_identity_contract(&workflow).unwrap();
}

#[derive(Clone, Copy)]
enum Exchange {
    Token,
    Empty,
    Rejected,
}

type Fixup<'a> = &'a dyn Fn(&Path);

/// One run of a publishing job's steps against a tagged fixture checkout.
struct Run<'a> {
    workflow: Value,
    job: &'a str,
    event: &'a str,
    git_ref: &'a str,
    exchange: Exchange,
    index_status: &'a str,
    /// The index body; `{local}` stands for the checksum of the local package.
    index_body: String,
    /// Changes the release commit before the fixture records its identity.
    release: Fixup<'a>,
    /// Changes the checkout after the fixture records its identity.
    checkout: Fixup<'a>,
    /// The commit the package records, when it is not the checkout HEAD.
    vcs_commit: Option<&'a str>,
}

impl Default for Run<'_> {
    fn default() -> Self {
        Run {
            workflow: workflow(),
            job: "crates-recovery",
            event: "workflow_dispatch",
            git_ref: "refs/heads/main",
            exchange: Exchange::Token,
            index_status: "404",
            index_body: String::new(),
            release: &|_| {},
            checkout: &|_| {},
            vcs_commit: None,
        }
    }
}

impl Run<'_> {
    fn push() -> Self {
        Run {
            job: "crates",
            event: "push",
            git_ref: "refs/tags/0.4.0",
            ..Run::default()
        }
    }
}

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
        ])
        .args(["-c", "commit.gpgSign=false", "-c", "tag.gpgSign=false"])
        .args(args)
        .current_dir(repo)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", repo)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn retag(repo: &Path) {
    git(
        repo,
        &["tag", "-f", "-a", "-m", "release", RECOVERY_VERSION, "HEAD"],
    );
}

// Records each call. `cargo package` packs the checkout like Cargo does,
// recording HEAD in `.cargo_vcs_info.json`, with fixed archive metadata.
const CARGO_STUB: &str = r#"#!/bin/bash
set -euo pipefail
version="$(sed -nE 's/^version = "([^"]+)"/\1/p' Cargo.toml | head -1)"
case "$*" in
  'metadata --locked --no-deps --format-version 1')
    printf '{"packages":[{"name":"toha","version":"%s"}]}\n' "$version" ;;
  'package --locked')
    echo package >> "$TRACE"
    stage="$(mktemp -d)/toha-$version"
    mkdir -p "$stage" target/package
    printf '{"git":{"sha1":"%s"},"path_in_vcs":""}\n' "${VCS_COMMIT:-$(git rev-parse HEAD)}" \
      > "$stage/.cargo_vcs_info.json"
    cp Cargo.toml Cargo.lock "$stage/"
    tar --mtime=@0 --owner=0 --group=0 --numeric-owner --sort=name \
      -C "$(dirname "$stage")" -cf - "toha-$version" | gzip -n > "target/package/toha-$version.crate" ;;
  'publish --locked')
    echo "publish token=${CARGO_REGISTRY_TOKEN:-absent}" >> "$TRACE" ;;
  *) echo "unexpected cargo $*" >&2; exit 1 ;;
esac
"#;

const CURL_STUB: &str = r#"#!/bin/bash
set -euo pipefail
echo lookup >> "$TRACE"
local="$(sha256sum target/package/toha-*.crate | cut -d' ' -f1)"
while (( $# )); do
  if [[ $1 == --output ]]; then sed "s/{local}/$local/" "$INDEX_BODY" > "$2"; shift; fi
  shift
done
printf '%s' "$INDEX_STATUS"
"#;

/// Runs the job's steps in order. Actions are simulated; each `run` script is
/// the workflow's own text, run the way GitHub runs `shell: bash`. Returns the
/// result and the trace of package, auth, lookup and publish calls.
fn run_publication(run: &Run) -> (Result<(), String>, Vec<String>) {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path();
    let repo = root.join("repo");
    fs::create_dir_all(repo.join("scripts/release")).unwrap();
    fs::write(
        repo.join("Cargo.toml"),
        format!("[package]\nname = \"toha\"\nversion = \"{RECOVERY_VERSION}\"\n"),
    )
    .unwrap();
    fs::write(repo.join("Cargo.lock"), "version = 4\n").unwrap();
    fs::write(repo.join(".gitignore"), "/target/\n").unwrap();
    executable(
        &repo.join("scripts/release/ensure-crate.sh"),
        include_str!("../scripts/release/ensure-crate.sh"),
    );
    git(&repo, &["init", "--quiet", "--initial-branch=main"]);
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "--quiet", "-m", "release"]);
    retag(&repo);
    (run.release)(&repo);
    let commit = git(&repo, &["rev-parse", "HEAD"]);
    let tag_object = git(&repo, &["rev-parse", RECOVERY_VERSION]);
    (run.checkout)(&repo);

    let bin = root.join("bin");
    fs::create_dir(&bin).unwrap();
    executable(&bin.join("cargo"), CARGO_STUB);
    executable(&bin.join("curl"), CURL_STUB);
    fs::write(root.join("index"), &run.index_body).unwrap();
    let trace = root.join("trace");
    fs::write(&trace, "").unwrap();
    let outcome = |result| {
        let calls = fs::read_to_string(&trace).unwrap();
        (result, calls.lines().map(str::to_owned).collect())
    };

    let mut token = None;
    for step in steps(&run.workflow, run.job) {
        if let Some(action) = step["uses"].as_str() {
            if action == AUTH_ACTION {
                fs::write(&trace, fs::read_to_string(&trace).unwrap() + "auth\n").unwrap();
                token = match run.exchange {
                    Exchange::Token => Some("fixture-temporary-token"),
                    Exchange::Empty => Some(""),
                    Exchange::Rejected => {
                        return outcome(Err("auth: crates.io rejected the exchange".into()));
                    }
                };
            } else if !action.starts_with("actions/checkout@")
                && !action.starts_with("dtolnay/rust-toolchain@")
            {
                return outcome(Err(format!("unsupported action: {action}")));
            }
            continue;
        }
        let script = step["run"].as_str().unwrap();
        assert!(
            !script.contains("${{"),
            "run scripts take inputs through env"
        );
        let mut command = Command::new("bash");
        command
            .args(["--noprofile", "--norc", "-eo", "pipefail", "-c", script])
            .current_dir(&repo)
            .env_clear()
            .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
            .env("HOME", root)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("TRACE", &trace)
            .env("INDEX_BODY", root.join("index"))
            .env("INDEX_STATUS", run.index_status)
            .env("GITHUB_EVENT_NAME", run.event)
            .env("GITHUB_REF", run.git_ref)
            .env("GITHUB_REF_NAME", run.git_ref.rsplit('/').next().unwrap());
        if let Some(vcs_commit) = run.vcs_commit {
            command.env("VCS_COMMIT", vcs_commit);
        }
        for (name, value) in step["env"].as_object().into_iter().flatten() {
            let value = match value.as_str().unwrap() {
                AUTH_TOKEN => token.expect("the token step runs after auth").to_owned(),
                // A fixture cannot recreate the real object IDs; the identity
                // contract pins the declared values.
                RECOVERY_COMMIT => commit.clone(),
                RECOVERY_TAG_OBJECT => tag_object.clone(),
                other if other.contains("${{") => panic!("unsupported step env: {other}"),
                other => other.to_owned(),
            };
            command.env(name, value);
        }
        let output = command.output().unwrap();
        if !output.status.success() {
            return outcome(Err(format!(
                "{}: {}{}",
                step["name"].as_str().unwrap_or("step"),
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )));
        }
    }
    outcome(Ok(()))
}

fn index_line(version: &str, checksum: &str) -> String {
    format!("{{\"name\":\"toha\",\"vers\":\"{version}\",\"cksum\":\"{checksum}\"}}\n")
}

const VERIFIED: [&str; 4] = ["package", "auth", "package", "lookup"];
const PUBLISHED: [&str; 5] = [
    "package",
    "auth",
    "package",
    "lookup",
    "publish token=fixture-temporary-token",
];

#[test]
fn recovery_publishes_the_tagged_crate_from_a_main_dispatch() {
    // GITHUB_REF_NAME is `main`; ensure-crate.sh still gets 0.4.0.
    let (result, trace) = run_publication(&Run::default());
    result.unwrap();
    assert_eq!(trace, PUBLISHED);
    let (result, trace) = run_publication(&Run {
        index_status: "200",
        index_body: index_line("0.3.0", &"0".repeat(64)),
        ..Run::default()
    });
    result.unwrap();
    assert_eq!(trace, PUBLISHED, "an index without 0.4.0 publishes");
}

#[test]
fn push_publication_authenticates_through_trusted_publishing() {
    let (result, trace) = run_publication(&Run::push());
    result.unwrap();
    assert_eq!(trace, PUBLISHED);
}

#[test]
fn recovery_follows_the_index_checksum() {
    let (result, trace) = run_publication(&Run {
        index_status: "200",
        index_body: index_line(RECOVERY_VERSION, "{local}"),
        ..Run::default()
    });
    result.unwrap();
    assert_eq!(trace, VERIFIED, "matching bytes skip publication");
    for (status, body, expected) in [
        (
            "200",
            index_line(RECOVERY_VERSION, &"f".repeat(64)),
            "different package bytes",
        ),
        ("500", String::new(), "failed with HTTP 500"),
    ] {
        let (result, trace) = run_publication(&Run {
            index_status: status,
            index_body: body,
            ..Run::default()
        });
        let error = result.unwrap_err();
        assert!(error.contains(expected), "{error}");
        assert_eq!(trace, VERIFIED, "{expected}: no publication");
    }
}

#[test]
fn a_failed_exchange_stops_before_publication() {
    for base in [Run::default(), Run::push()] {
        let job = base.job;
        let (result, trace) = run_publication(&Run {
            exchange: Exchange::Rejected,
            ..base
        });
        assert!(result.unwrap_err().starts_with("auth:"), "{job}");
        assert_eq!(trace, ["package", "auth"], "{job}: rejected exchange");
    }
    for base in [Run::default(), Run::push()] {
        let job = base.job;
        let (result, trace) = run_publication(&Run {
            exchange: Exchange::Empty,
            ..base
        });
        let error = result.unwrap_err();
        assert!(
            error.contains("returned no temporary token"),
            "{job}: {error}"
        );
        assert_eq!(trace, ["package", "auth"], "{job}: missing token");
    }
}

/// Each case breaks one part of the recovery identity and names the refusal.
fn identity_refusals(workflow: &Value) -> Result<(), String> {
    let none: Fixup = &|_| {};
    let later: Fixup = &|repo| {
        git(repo, &["commit", "--quiet", "--allow-empty", "-m", "later"]);
    };
    let lightweight: Fixup = &|repo| {
        git(repo, &["tag", "-f", RECOVERY_VERSION, "HEAD"]);
    };
    let bumped: Fixup = &|repo| {
        fs::write(
            repo.join("Cargo.toml"),
            "[package]\nname = \"toha\"\nversion = \"0.4.1\"\n",
        )
        .unwrap();
        git(repo, &["commit", "--quiet", "--amend", "--no-edit", "-a"]);
        retag(repo);
    };
    let unlocked: Fixup = &|repo| {
        fs::write(repo.join(".gitignore"), "/target/\n/Cargo.lock\n").unwrap();
        git(repo, &["rm", "--quiet", "--cached", "Cargo.lock"]);
        git(repo, &["commit", "--quiet", "--amend", "--no-edit", "-a"]);
        retag(repo);
    };
    let modified: Fixup = &|repo| fs::write(repo.join("Cargo.lock"), "changed\n").unwrap();
    let untracked: Fixup = &|repo| fs::write(repo.join("extra.rs"), "").unwrap();
    let cases: [(&str, Fixup, Fixup, Option<&str>, &str); 9] = [
        ("moved HEAD", none, later, None, "HEAD is not"),
        (
            "lightweight tag",
            none,
            lightweight,
            None,
            "is not an annotated tag",
        ),
        (
            "replaced tag object",
            none,
            &|repo| {
                git(
                    repo,
                    &["tag", "-f", "-a", "-m", "replaced", RECOVERY_VERSION],
                );
            },
            None,
            "is not tag object",
        ),
        (
            "tag on another commit",
            later,
            none,
            None,
            "does not point to",
        ),
        (
            "manifest version",
            bumped,
            none,
            None,
            "declares 0.4.1, not 0.4.0",
        ),
        (
            "uncommitted lockfile",
            unlocked,
            none,
            None,
            "Cargo.lock is not committed",
        ),
        (
            "modified source",
            none,
            modified,
            None,
            "checkout is not clean",
        ),
        (
            "untracked source",
            none,
            untracked,
            None,
            "checkout is not clean",
        ),
        (
            "package commit",
            none,
            none,
            Some("0123456789abcdef0123456789abcdef01234567"),
            "The package records commit",
        ),
    ];
    for (case, release, checkout, vcs_commit, expected) in cases {
        let (result, trace) = run_publication(&Run {
            workflow: workflow.clone(),
            release,
            checkout,
            vcs_commit,
            ..Run::default()
        });
        match result {
            Err(error) if error.contains(expected) => {}
            other => return Err(format!("{case} must refuse with {expected:?}: {other:?}")),
        }
        if trace.iter().any(|call| call == "auth") {
            return Err(format!("{case} reached authentication: {trace:?}"));
        }
    }
    Ok(())
}

#[test]
fn each_wrong_recovery_identity_refuses_before_authentication() {
    identity_refusals(&workflow()).unwrap();
    let (result, trace) = run_publication(&Run {
        git_ref: "refs/heads/feature",
        ..Run::default()
    });
    let error = result.unwrap_err();
    assert!(error.contains("Dispatch the recovery from main"), "{error}");
    assert!(trace.is_empty(), "{trace:?}");
}

#[test]
fn crate_publication_controls_expose_bypassed_guards() {
    // A selector widened with `||` runs the recovery on every dispatch.
    let mut widened = workflow();
    widened["jobs"]["crates-recovery"]["if"] =
        format!("github.event_name == 'workflow_dispatch' || inputs.mode == '{RECOVERY_MODE}'")
            .into();
    let error = selector_contract(&widened).unwrap_err();
    assert!(
        error.contains("a build-only dispatch must only build"),
        "{error}"
    );

    // A stored token as fallback for the exchange output.
    let mut fallback = workflow();
    let publish = fallback["jobs"]["crates"]["steps"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|step| step["name"] == "Publish or verify the crate")
        .unwrap();
    publish["env"]["CARGO_REGISTRY_TOKEN"] =
        "${{ steps.auth.outputs.token || secrets.CARGO_REGISTRY_TOKEN }}".into();
    let error = auth_contract(&fallback).unwrap_err();
    assert!(
        error.contains("must not use a stored crates.io token"),
        "{error}"
    );

    // Without the tag object check, a replaced 0.4.0 tag reaches authentication.
    let mut unchecked = workflow();
    let identity = unchecked["jobs"]["crates-recovery"]["steps"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|step| step["name"] == "Verify recovery source identity")
        .unwrap();
    let script = identity["run"].as_str().unwrap();
    let guard = "[[ \"$(git rev-parse \"$tag\")\" == \"$RECOVERY_TAG_OBJECT\" ]] ||\n  fail \"$tag is not tag object $RECOVERY_TAG_OBJECT.\"\n";
    assert_eq!(script.matches(guard).count(), 1, "tag object guard site");
    identity["run"] = script.replace(guard, "").into();
    let error = identity_refusals(&unchecked).unwrap_err();
    assert!(
        error.starts_with("replaced tag object must refuse"),
        "{error}"
    );
}
