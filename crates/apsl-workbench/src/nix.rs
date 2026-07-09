
use std::collections::HashSet;
use std::process::Command;

const NIX_CONTAINER: &str = "nixplay";
fn dyn_host_dir() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/home/devkit".into());
    format!("{home}/.apsl/nix-dyn")
}

pub struct BuildResult {
    pub app: String,
    pub manifest: std::collections::HashMap<String, String>,
    pub closure: HashSet<String>,
}

fn sh(cmd: &str) -> std::io::Result<std::process::Output> {
    Command::new("docker")
        .args(["exec", NIX_CONTAINER, "sh", "-c", cmd])
        .output()
}

pub fn request_id(src: &str) -> String {
    apsl_core::hash::sha256_hex(src.as_bytes())[..12].to_string()
}

pub fn gen_flake(nodes: &[String]) -> String {
    let mk: String = nodes
        .iter()
        .map(|n| {
            format!(
                "        \"{n}\" = mkNode \"{n}\" \"#!/bin/sh\\necho apsl-node-{n}\\n\";"
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let manifest_entries: String = nodes
        .iter()
        .map(|n| format!("\\\"{n}\\\":\\\"${{nodes.{n}}}\\\""))
        .collect::<Vec<_>>()
        .join(",");
    let cps: String = nodes
        .iter()
        .map(|n| format!("        cp ${{nodes.{n}}}/bin/{n} $out/bin/"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        r#"{{
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-24.05";
  outputs = {{ self, nixpkgs }}:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs {{ inherit system; }};
      mkNode = name: body: pkgs.runCommand "apsl-node-${{name}}" {{ }} ''
        mkdir -p $out/bin; printf '%s' "${{body}}" > $out/bin/${{name}}; chmod +x $out/bin/${{name}}
      '';
      nodes = {{
{mk}
      }};
      app = pkgs.runCommand "apsl-app" {{ }} ''
        mkdir -p $out/bin
{cps}
        printf '{{{manifest_entries}}}' > $out/manifest.json
      '';
    in {{ packages.${{system}} = nodes // {{ inherit app; }}; }};
}}"#
    )
}

pub fn build(src: &str, nodes: &[String]) -> Result<BuildResult, String> {
    let rid = request_id(src);
    let dir = format!("{}/{}", dyn_host_dir(), rid);
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir {dir}: {e}"))?;
    std::fs::write(format!("{dir}/flake.nix"), gen_flake(nodes))
        .map_err(|e| format!("write flake: {e}"))?;

    let app_out = sh(&format!(
        "cd /work/research/nix-dyn/{rid} && nix build \"path:.#app\" --no-link --print-out-paths 2>/dev/null | tail -1"
    ))
    .map_err(|e| format!("docker exec: {e}"))?;
    let app = String::from_utf8_lossy(&app_out.stdout).trim().to_string();
    if app.is_empty() {
        let err = sh(&format!(
            "cd /work/research/nix-dyn/{rid} && nix build \"path:.#app\" --no-link 2>&1 | tail -20"
        ))
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
        return Err(format!("nix build failed: {}", &err[..err.len().min(600)]));
    }

    let manifest_raw = sh(&format!("cat {app}/manifest.json"))
        .map_err(|e| format!("cat manifest: {e}"))?;
    let manifest: std::collections::HashMap<String, String> =
        serde_json::from_slice(&manifest_raw.stdout)
            .map_err(|e| format!("parse manifest: {e}"))?;

    let closure_raw = sh(&format!("nix-store -q --requisites {app}"))
        .map_err(|e| format!("nix-store -q: {e}"))?;
    let closure: HashSet<String> = String::from_utf8_lossy(&closure_raw.stdout)
        .split_whitespace()
        .map(str::to_string)
        .collect();

    Ok(BuildResult {
        app,
        manifest,
        closure,
    })
}

pub fn store_basename(store_path: &str) -> String {
    store_path
        .rsplit('/')
        .next()
        .unwrap_or(store_path)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flake_contains_each_node_and_app() {
        let f = gen_flake(&["alpha".into(), "beta".into()]);
        assert!(f.contains("\"alpha\" = mkNode \"alpha\""), "{f}");
        assert!(f.contains("\"beta\" = mkNode \"beta\""), "{f}");
        assert!(f.contains("apsl-app"), "{f}");
        assert!(f.contains("\\\"alpha\\\":\\\"${nodes.alpha}\\\""), "{f}");
    }

    #[test]
    fn request_id_is_stable_and_short() {
        assert_eq!(request_id("hello"), request_id("hello"));
        assert_eq!(request_id("hello").len(), 12);
        assert_ne!(request_id("a"), request_id("b"));
    }

    #[test]
    fn store_basename_strips_dirs() {
        assert_eq!(
            store_basename("/nix/store/abc123-apsl-node-x"),
            "abc123-apsl-node-x"
        );
    }
}
