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

