use serde_json::Value;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use tokio::fs;
use tokio::sync::Mutex;

use super::godot;
use crate::constants::GIT_BIN;
use crate::state::{AppState, ProjectSize};
use crate::tools::{ToolResult, text_err, text_ok};
use crate::util::{run_git, validate_name};

pub async fn run(state: Arc<Mutex<AppState>>, args: &Value) -> ToolResult {
    let name = args
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| (-32602i32, "Missing required argument: name".to_string()))?
        .to_string();

    if let Err(reason) = validate_name(&name) {
        return Ok(text_err(format!("400: invalid project name — {reason}")));
    }

    let language = args
        .get("language")
        .and_then(|v| v.as_str())
        .ok_or_else(|| (-32602i32, "Missing required argument: language".to_string()))?
        .to_string();

    let size_str = args
        .get("size")
        .and_then(|v| v.as_str())
        .unwrap_or("medium")
        .to_string();

    let size = ProjectSize::from_str(&size_str).unwrap_or_default();
    let max_files = size.max_files();
    let max_depth = size.max_depth(&language);

    let base_dir = {
        let st = state.lock().await;
        st.base_dir.clone()
    };

    let project_dir = base_dir.join(&name);

    if project_dir.exists() {
        return Ok(text_err(format!(
            "Project directory already exists: {name}"
        )));
    }

    if let Err(e) = scaffold_project(
        &project_dir,
        &name,
        &language,
        &size_str,
        max_files,
        max_depth,
    )
    .await
    {
        // Clean up on failure
        let _ = fs::remove_dir_all(&project_dir).await;
        return Ok(text_err(format!("Failed to create project: {e}")));
    }

    {
        let mut st = state.lock().await;
        st.project_dir = Some(project_dir);
        st.language = Some(language.to_lowercase());
        st.max_files = max_files;
        st.max_depth = max_depth;
        st.size = size;
        st.project_name = Some(name.clone());
    }

    Ok(text_ok(format!(
        "Project '{name}' created (language={language}, size={size_str}, max_files={max_files}, max_depth={max_depth})."
    )))
}

// ── scaffolding ───────────────────────────────────────────────────────────────
// SUCRITY NOTE: Commands usage table - USER MUST review their PATH variables to ensure these bindings are safe
//   ┌──────────┬───────────────────────────────────┬─────────────────────────────┐
//   │ Language │           build.sh uses           │         run.sh uses         │
//   ├──────────┼───────────────────────────────────┼─────────────────────────────┤
//   │ Rust     │ cargo                             │ cargo                       │
//   ├──────────┼───────────────────────────────────┼─────────────────────────────┤
//   │ Go       │ go                                │ go                          │
//   ├──────────┼───────────────────────────────────┼─────────────────────────────┤
//   │ Java     │ mvn                               │ mvn                         │
//   ├──────────┼───────────────────────────────────┼─────────────────────────────┤
//   │ Python   │ python3, .venv/bin/pip (relative) │ .venv/bin/python / python3  │
//   ├──────────┼───────────────────────────────────┼─────────────────────────────┤
//   │ C        │ make → then Makefile uses gcc, rm │ the built binary (relative) │
//   ├──────────┼───────────────────────────────────┼─────────────────────────────┤
//   │ C++      │ cmake, optionally ninja           │ built binary (relative)     │
//   ├──────────┼───────────────────────────────────┼─────────────────────────────┤
//   │ JS/Node  │ npm                               │ npm                         │
//   ├──────────┼───────────────────────────────────┼─────────────────────────────┤
//   │ TS       │ npm, npx (tsc via npx)            │ node                        │
//   ├──────────┼───────────────────────────────────┼─────────────────────────────┤
//   │ Godot    │ $GODOT env or godot               │ same                        │
//   └──────────┴───────────────────────────────────┴─────────────────────────────┘
async fn scaffold_project(
    project_dir: &PathBuf,
    name: &str,
    language: &str,
    size_str: &str,
    max_files: usize,
    max_depth: usize,
) -> Result<(), String> {
    let parent = project_dir
        .parent()
        .ok_or_else(|| "Invalid project path".to_string())?;

    match language.to_lowercase().as_str() {
        "rust" => {
            // --vcs none: we manage git ourselves for consistency
            let status = tokio::process::Command::new("cargo")
                .args(["new", "--vcs", "none", name])
                .current_dir(parent)
                .status()
                .await
                .map_err(|e| format!("cargo new: {e}"))?;
            if !status.success() {
                return Err("cargo new failed".to_string());
            }
        }
        "go" => {
            fs::create_dir_all(project_dir)
                .await
                .map_err(|e| e.to_string())?;
            let ok = tokio::process::Command::new("go")
                .args(["mod", "init", name])
                .current_dir(project_dir)
                .status()
                .await
                .map_err(|e| format!("go mod init: {e}"))?
                .success();
            if !ok {
                return Err("go mod init failed".to_string());
            }
            fs::write(project_dir.join("main.go"), go_main(name))
                .await
                .map_err(|e| e.to_string())?;
        }
        "java" => {
            let pkg_id = java_pkg_id(name);
            let src = project_dir.join("src/main/java/com/example").join(&pkg_id);
            fs::create_dir_all(&src).await.map_err(|e| e.to_string())?;
            fs::create_dir_all(project_dir.join("src/main/resources"))
                .await
                .map_err(|e| e.to_string())?;
            fs::create_dir_all(project_dir.join("src/test/java/com/example").join(&pkg_id))
                .await
                .map_err(|e| e.to_string())?;
            let pkg = format!("com.example.{pkg_id}");
            let class = to_class_name(name);
            fs::write(src.join("Main.java"), java_main(&pkg, &class))
                .await
                .map_err(|e| e.to_string())?;
            fs::write(project_dir.join("pom.xml"), pom_xml(name))
                .await
                .map_err(|e| e.to_string())?;
        }
        "python" => {
            let pkg = name.to_lowercase().replace('-', "_");
            fs::create_dir_all(project_dir.join(&pkg))
                .await
                .map_err(|e| e.to_string())?;
            fs::write(project_dir.join(&pkg).join("__init__.py"), "")
                .await
                .map_err(|e| e.to_string())?;
            fs::write(project_dir.join("main.py"), PYTHON_MAIN)
                .await
                .map_err(|e| e.to_string())?;
            fs::write(project_dir.join("requirements.txt"), "")
                .await
                .map_err(|e| e.to_string())?;
        }
        "c" => {
            fs::create_dir_all(project_dir.join("src"))
                .await
                .map_err(|e| e.to_string())?;
            fs::create_dir_all(project_dir.join("include"))
                .await
                .map_err(|e| e.to_string())?;
            fs::write(project_dir.join("src/main.c"), C_MAIN)
                .await
                .map_err(|e| e.to_string())?;
            fs::write(project_dir.join("Makefile"), c_makefile(name))
                .await
                .map_err(|e| e.to_string())?;
        }
        "c++" | "cpp" => {
            fs::create_dir_all(project_dir.join("src"))
                .await
                .map_err(|e| e.to_string())?;
            fs::create_dir_all(project_dir.join("include"))
                .await
                .map_err(|e| e.to_string())?;
            fs::write(project_dir.join("src/main.cpp"), CPP_MAIN)
                .await
                .map_err(|e| e.to_string())?;
            fs::write(project_dir.join("CMakeLists.txt"), cmake_lists(name))
                .await
                .map_err(|e| e.to_string())?;
        }
        "javascript" | "js" | "node" => {
            fs::create_dir_all(project_dir.join("src"))
                .await
                .map_err(|e| e.to_string())?;
            fs::write(project_dir.join("src/index.js"), JS_MAIN)
                .await
                .map_err(|e| e.to_string())?;
            fs::write(
                project_dir.join("package.json"),
                npm_package_json(name, "src/index.js"),
            )
            .await
            .map_err(|e| e.to_string())?;
        }
        "typescript" | "ts" => {
            fs::create_dir_all(project_dir.join("src"))
                .await
                .map_err(|e| e.to_string())?;
            fs::write(project_dir.join("src/index.ts"), TS_MAIN)
                .await
                .map_err(|e| e.to_string())?;
            fs::write(
                project_dir.join("package.json"),
                npm_package_json(name, "dist/index.js"),
            )
            .await
            .map_err(|e| e.to_string())?;
            fs::write(project_dir.join("tsconfig.json"), TSCONFIG)
                .await
                .map_err(|e| e.to_string())?;
        }
        "godot" | "godot3d" | "godot-3d" => {
            fs::create_dir_all(project_dir)
                .await
                .map_err(|e| e.to_string())?;
            godot::scaffold_3d(project_dir, name).await?;
        }
        "godot2d" | "godot-2d" => {
            fs::create_dir_all(project_dir)
                .await
                .map_err(|e| e.to_string())?;
            godot::scaffold_2d(project_dir, name).await?;
        }
        _ => {
            fs::create_dir_all(project_dir)
                .await
                .map_err(|e| e.to_string())?;
            fs::write(
                project_dir.join("README.md"),
                format!("# {name}\n\nLanguage: {language}\n"),
            )
            .await
            .map_err(|e| e.to_string())?;
        }
    }

    // ── build.sh / run.sh ─────────────────────────────────────────────────────
    write_scripts(project_dir, language, name).await?;

    // ── git init ──────────────────────────────────────────────────────────────
    let ok = tokio::process::Command::new(GIT_BIN)
        .args(["init"])
        .current_dir(project_dir)
        .status()
        .await
        .map_err(|e| format!("git init: {e}"))?
        .success();
    if !ok {
        return Err("git init failed".to_string());
    }

    // Configure a local identity so commits always work
    // for (k, v) in [
    //     ("user.email", "mcp@file-mcp.local"),
    //     ("user.name", "MCP Server"),
    // ] {
    //     let _ = run_git(Path::new(GIT_BIN), project_dir, &["config", k, v]).await;
    // }

    // Language-appropriate .gitignore
    let gi = gitignore_for(language);
    if !gi.is_empty() {
        fs::write(project_dir.join(".gitignore"), gi)
            .await
            .map_err(|e| e.to_string())?;
    }

    // .meta/project.json
    let meta_dir = project_dir.join(".meta");
    fs::create_dir_all(&meta_dir)
        .await
        .map_err(|e| e.to_string())?;
    let meta = serde_json::json!({
        "name": name,
        "language": language.to_lowercase(),
        "size": size_str,
        "max_files": max_files,
        "max_depth": max_depth
    });
    fs::write(
        meta_dir.join("project.json"),
        serde_json::to_string_pretty(&meta).unwrap(),
    )
    .await
    .map_err(|e| e.to_string())?;

    // Initial commit
    let _ = run_git(Path::new(GIT_BIN), project_dir, &["add", "-A"]).await;
    let _ = run_git(
        Path::new(GIT_BIN),
        project_dir,
        &["commit", "-m", "initial-commit"],
    )
    .await;

    Ok(())
}

// ── language helpers ──────────────────────────────────────────────────────────

fn java_pkg_id(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

fn to_class_name(name: &str) -> String {
    name.split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|s| {
            let mut ch = s.chars();
            match ch.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + ch.as_str(),
            }
        })
        .collect()
}

fn go_main(name: &str) -> String {
    format!(
        "package main\n\nimport \"fmt\"\n\nfunc main() {{\n\tfmt.Println(\"Hello from {name}!\")\n}}\n"
    )
}

fn java_main(pkg: &str, class: &str) -> String {
    format!(
        "package {pkg};\n\npublic class {class} {{\n    public static void main(String[] args) {{\n        System.out.println(\"Hello, World!\");\n    }}\n}}\n"
    )
}

fn pom_xml(name: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0"
         xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
         xsi:schemaLocation="http://maven.apache.org/POM/4.0.0 http://maven.apache.org/xsd/maven-4.0.0.xsd">
    <modelVersion>4.0.0</modelVersion>
    <groupId>com.example</groupId>
    <artifactId>{name}</artifactId>
    <version>1.0-SNAPSHOT</version>
    <properties>
        <maven.compiler.source>17</maven.compiler.source>
        <maven.compiler.target>17</maven.compiler.target>
        <project.build.sourceEncoding>UTF-8</project.build.sourceEncoding>
    </properties>
</project>
"#
    )
}

fn c_makefile(name: &str) -> String {
    format!(
        "CC      = gcc\nCFLAGS  = -Wall -Wextra -Iinclude\nSRC     = $(wildcard src/*.c)\nOBJ     = $(SRC:.c=.o)\nTARGET  = {name}\n\nall: $(TARGET)\n\n$(TARGET): $(OBJ)\n\t$(CC) -o $@ $^\n\n%.o: %.c\n\t$(CC) $(CFLAGS) -c -o $@ $<\n\nclean:\n\trm -f $(OBJ) $(TARGET)\n\n.PHONY: all clean\n"
    )
}

fn cmake_lists(name: &str) -> String {
    format!(
        "cmake_minimum_required(VERSION 3.14)\nproject({name})\n\nset(CMAKE_CXX_STANDARD 17)\n\nadd_executable({name} src/main.cpp)\ntarget_include_directories({name} PRIVATE include)\n"
    )
}

fn npm_package_json(name: &str, main: &str) -> String {
    format!(
        "{{\n  \"name\": \"{name}\",\n  \"version\": \"0.1.0\",\n  \"main\": \"{main}\",\n  \"scripts\": {{\n    \"start\": \"node {main}\"\n  }}\n}}\n"
    )
}

fn gitignore_for(lang: &str) -> &'static str {
    match lang.to_lowercase().as_str() {
        "rust" => "/target\n",
        "go" => "*.exe\n*.test\n*.out\nvendor/\n",
        "java" => "*.class\n*.jar\ntarget/\n.idea/\n*.iml\n",
        "python" => "__pycache__/\n*.pyc\n*.pyo\n.venv/\nvenv/\ndist/\nbuild/\n*.egg-info/\n",
        "c" | "c++" | "cpp" => "*.o\n*.a\n*.so\nbuild/\n",
        "javascript" | "js" | "node" | "typescript" | "ts" => {
            "node_modules/\ndist/\nbuild/\n*.log\n"
        }
        "godot" | "godot3d" | "godot-3d" | "godot2d" | "godot-2d" => godot::GITIGNORE,
        _ => "",
    }
}

// ── build.sh / run.sh ────────────────────────────────────────────────────────

async fn write_scripts(dir: &PathBuf, language: &str, name: &str) -> Result<(), String> {
    let (build, run) = scripts_for(language, name);
    for (filename, content) in [("build.sh", build), ("run.sh", run)] {
        let path = dir.join(filename);
        fs::write(&path, content)
            .await
            .map_err(|e| format!("{filename}: {e}"))?;
        // Make executable on Unix.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&path)
                .map_err(|e| e.to_string())?
                .permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&path, perms).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn scripts_for(language: &str, name: &str) -> (String, String) {
    match language.to_lowercase().as_str() {
        "rust" => (
            format!(
                "#!/usr/bin/env bash\nset -euo pipefail\ncargo build --release\necho \"Binary: target/release/{name}\"\n"
            ),
            "#!/usr/bin/env bash\nset -euo pipefail\ncargo run\n".into(),
        ),
        "go" => (
            format!(
                "#!/usr/bin/env bash\nset -euo pipefail\ngo build -o {name} .\necho \"Binary: ./{name}\"\n"
            ),
            "#!/usr/bin/env bash\nset -euo pipefail\ngo run .\n".into(),
        ),
        "java" => {
            let pkg_id = java_pkg_id(name);
            let class = to_class_name(name);
            (
                "#!/usr/bin/env bash\nset -euo pipefail\nmvn package -q\necho \"JAR ready in target/\"\n".into(),
                format!(
                    "#!/usr/bin/env bash\nset -euo pipefail\nmvn -q compile\nmvn -q exec:java -Dexec.mainClass=\"com.example.{pkg_id}.{class}\"\n"
                ),
            )
        }
        "python" => (
            "#!/usr/bin/env bash\nset -euo pipefail\nif [ ! -d \".venv\" ]; then\n    python3 -m venv .venv\nfi\n.venv/bin/pip install -q -r requirements.txt\necho \"Virtual environment ready.\"\n".into(),
            "#!/usr/bin/env bash\nset -euo pipefail\nif [ -x \".venv/bin/python\" ]; then\n    exec .venv/bin/python main.py\nelse\n    exec python3 main.py\nfi\n".into(),
        ),
        "c" => (
            format!("#!/usr/bin/env bash\nset -euo pipefail\nmake\necho \"Binary: ./{name}\"\n"),
            format!("#!/usr/bin/env bash\nset -euo pipefail\n[ -f \"{name}\" ] || bash build.sh\nexec ./{name}\n"),
        ),
        "c++" | "cpp" => (
            format!(
                "#!/usr/bin/env bash\nset -euo pipefail\ncmake -B build -DCMAKE_BUILD_TYPE=Release -S . -Wno-dev -G Ninja 2>/dev/null \\\n    || cmake -B build -DCMAKE_BUILD_TYPE=Release -S . -Wno-dev\ncmake --build build --parallel\necho \"Binary: build/{name}\"\n"
            ),
            format!(
                "#!/usr/bin/env bash\nset -euo pipefail\n[ -f \"build/{name}\" ] || bash build.sh\nexec ./build/{name}\n"
            ),
        ),
        "javascript" | "js" | "node" => (
            "#!/usr/bin/env bash\nset -euo pipefail\nnpm install\n".into(),
            "#!/usr/bin/env bash\nset -euo pipefail\nnpm start\n".into(),
        ),
        "typescript" | "ts" => (
            "#!/usr/bin/env bash\nset -euo pipefail\nnpm install\nnpx tsc\necho \"Compiled to dist/\"\n".into(),
            "#!/usr/bin/env bash\nset -euo pipefail\n[ -d \"dist\" ] || bash build.sh\nexec node dist/index.js\n".into(),
        ),
        "godot" | "godot3d" | "godot-3d" | "godot2d" | "godot-2d" => (
            // build = headless import (asset import pass Godot needs before exporting)
            "#!/usr/bin/env bash\nset -euo pipefail\nGODOT=\"${GODOT:-godot}\"\necho \"==> Importing project assets (headless)...\"\n\"$GODOT\" --headless --path . --import 2>/dev/null || true\necho \"==> Done. Use the Godot editor (Editor > Export) to create a distributable build.\"\n".into(),
            // run = open project in Godot editor
            "#!/usr/bin/env bash\nset -euo pipefail\nGODOT=\"${GODOT:-godot}\"\necho \"==> Opening project in Godot editor...\"\nexec \"$GODOT\" --path .\n".into(),
        ),
        _ => (
            "#!/usr/bin/env bash\nset -euo pipefail\necho \"No build step configured for this project type.\"\n".into(),
            "#!/usr/bin/env bash\nset -euo pipefail\necho \"No run step configured for this project type.\"\n".into(),
        ),
    }
}

// ── embedded stubs ────────────────────────────────────────────────────────────

const PYTHON_MAIN: &str = r#"def main():
    print("Hello, World!")

if __name__ == "__main__":
    main()
"#;

const C_MAIN: &str = r#"#include <stdio.h>

int main(int argc, char *argv[]) {
    printf("Hello, World!\n");
    return 0;
}
"#;

const CPP_MAIN: &str = r#"#include <iostream>

int main(int argc, char *argv[]) {
    std::cout << "Hello, World!" << std::endl;
    return 0;
}
"#;

const JS_MAIN: &str = r#"'use strict';

function main() {
    console.log('Hello, World!');
}

main();
"#;

const TS_MAIN: &str = r#"function main(): void {
    console.log('Hello, World!');
}

main();
"#;

const TSCONFIG: &str = r#"{
  "compilerOptions": {
    "target": "ES2020",
    "module": "commonjs",
    "outDir": "./dist",
    "rootDir": "./src",
    "strict": true,
    "esModuleInterop": true
  },
  "include": ["src/**/*"],
  "exclude": ["node_modules", "dist"]
}
"#;
