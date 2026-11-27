use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JavaInstallation {
    pub path: PathBuf,
    pub version: String,
    pub major: u32,
    pub vendor: String,
}

/// Scan `JAVA_HOME`, `PATH`, and standard JDK/JRE/Minecraft runtime directories,
/// probe `java -version`, and return deduplicated installations sorted newest-first.
pub fn detect_java_installations() -> Vec<JavaInstallation> {
    let mut candidates = Vec::new();
    let exe_name = if cfg!(windows) { "java.exe" } else { "java" };

    // 1. JAVA_HOME
    if let Ok(java_home) = std::env::var("JAVA_HOME")
        && !java_home.trim().is_empty()
    {
        candidates.push(PathBuf::from(java_home).join("bin").join(exe_name));
    }

    // 2. PATH
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            candidates.push(dir.join(exe_name));
        }
    }

    // 3. Standard OS & Minecraft runtime locations
    collect_os_candidates(&mut candidates, exe_name);

    let mut seen = HashSet::new();
    let mut found = Vec::new();

    for candidate in candidates {
        if !candidate.is_file() {
            continue;
        }
        let canonical = std::fs::canonicalize(&candidate).unwrap_or_else(|_| candidate.clone());
        if !seen.insert(canonical.clone()) {
            continue;
        }
        if let Some(info) = probe_java(&canonical) {
            found.push(info);
        }
    }

    found.sort_by(|a, b| {
        b.major
            .cmp(&a.major)
            .then_with(|| b.version.cmp(&a.version))
            .then_with(|| a.path.cmp(&b.path))
    });
    found
}

fn collect_os_candidates(out: &mut Vec<PathBuf>, exe_name: &str) {
    #[cfg(target_os = "windows")]
    {
        let vendor_dirs = [
            "Java",
            "Eclipse Adoptium",
            "Adoptium",
            "AdoptOpenJDK",
            "Microsoft",
            "Zulu",
            "BellSoft",
            "Liberica",
            "Amazon Corretto",
            "Semeru",
            "SapMachine",
            "JetBrains",
        ];

        let mut base_roots = Vec::new();
        for env_key in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
            if let Ok(val) = std::env::var(env_key)
                && !val.is_empty()
            {
                base_roots.push(PathBuf::from(val));
            }
        }

        for root in &base_roots {
            for vendor in &vendor_dirs {
                let dir = root.join(vendor);
                scan_jdk_children(&dir, exe_name, out);
            }
        }

        // Minecraft Launcher bundled runtimes on Windows
        if let Ok(local_app) = std::env::var("LOCALAPPDATA") {
            let uwp_runtime = PathBuf::from(&local_app)
                .join("Packages")
                .join("Microsoft.4297127D64EC6_8wekyb3d8bbwe")
                .join("LocalCache")
                .join("Local")
                .join("runtime");
            scan_runtime_tree(&uwp_runtime, exe_name, 4, out);
        }
        if let Ok(pf86) = std::env::var("ProgramFiles(x86)") {
            let mc_runtime = PathBuf::from(pf86)
                .join("Minecraft Launcher")
                .join("runtime");
            scan_runtime_tree(&mc_runtime, exe_name, 4, out);
        }
        if let Ok(appdata) = std::env::var("APPDATA") {
            let dot_mc_runtime = PathBuf::from(appdata).join(".minecraft").join("runtime");
            scan_runtime_tree(&dot_mc_runtime, exe_name, 4, out);
        }
    }

    #[cfg(target_os = "macos")]
    {
        let mut jvm_roots = vec![
            PathBuf::from("/Library/Java/JavaVirtualMachines"),
            PathBuf::from("/opt/homebrew/opt"),
            PathBuf::from("/usr/local/opt"),
        ];
        if let Ok(home) = std::env::var("HOME") {
            let home_path = PathBuf::from(&home);
            jvm_roots.push(home_path.join("Library/Java/JavaVirtualMachines"));
            scan_runtime_tree(
                &home_path.join("Library/Application Support/minecraft/runtime"),
                exe_name,
                5,
                out,
            );
        }
        for root in jvm_roots {
            scan_jdk_children(&root, exe_name, out);
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let mut jvm_roots = vec![
            PathBuf::from("/usr/lib/jvm"),
            PathBuf::from("/usr/lib64/jvm"),
            PathBuf::from("/usr/java"),
            PathBuf::from("/opt/java"),
            PathBuf::from("/opt/jdk"),
        ];
        if let Ok(home) = std::env::var("HOME") {
            let home_path = PathBuf::from(&home);
            jvm_roots.push(home_path.join(".sdkman/candidates/java"));
            scan_runtime_tree(&home_path.join(".minecraft/runtime"), exe_name, 4, out);
        }
        for root in jvm_roots {
            scan_jdk_children(&root, exe_name, out);
        }
    }
}

fn scan_jdk_children(parent: &Path, exe_name: &str, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            out.push(p.join("bin").join(exe_name));
            out.push(p.join("Contents").join("Home").join("bin").join(exe_name));
        }
    }
}

fn scan_runtime_tree(dir: &Path, exe_name: &str, depth: usize, out: &mut Vec<PathBuf>) {
    if depth == 0 || !dir.is_dir() {
        return;
    }
    let direct = dir.join("bin").join(exe_name);
    if direct.is_file() {
        out.push(direct);
    }
    let mac_home = dir.join("Contents").join("Home").join("bin").join(exe_name);
    if mac_home.is_file() {
        out.push(mac_home);
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            scan_runtime_tree(&p, exe_name, depth - 1, out);
        }
    }
}

fn probe_java(path: &Path) -> Option<JavaInstallation> {
    let mut cmd = Command::new(path);
    cmd.arg("-version");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let output = cmd.output().ok()?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = if stderr.trim().is_empty() {
        stdout.into_owned()
    } else if stdout.trim().is_empty() {
        stderr.into_owned()
    } else {
        format!("{stderr}\n{stdout}")
    };

    let version = extract_quoted_version(&combined)?;
    let major = parse_java_major(&version)?;
    let vendor = infer_vendor(&combined);

    Some(JavaInstallation {
        path: path.to_path_buf(),
        version,
        major,
        vendor,
    })
}

fn extract_quoted_version(output: &str) -> Option<String> {
    for line in output.lines() {
        if line.contains("version")
            && let Some(start) = line.find('"')
            && let Some(end) = line[start + 1..].find('"')
        {
            return Some(line[start + 1..start + 1 + end].to_string());
        }
    }
    None
}

fn parse_java_major(version: &str) -> Option<u32> {
    let trimmed = version.trim();
    if let Some(rest) = trimmed.strip_prefix("1.") {
        let num_str: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        return num_str.parse().ok();
    }
    let num_str: String = trimmed.chars().take_while(|c| c.is_ascii_digit()).collect();
    num_str.parse().ok()
}

fn infer_vendor(output: &str) -> String {
    let lower = output.to_lowercase();
    if lower.contains("temurin") || lower.contains("adoptium") {
        "Eclipse Temurin".to_string()
    } else if lower.contains("microsoft") {
        "Microsoft".to_string()
    } else if lower.contains("zulu") || lower.contains("azul") {
        "Azul Zulu".to_string()
    } else if lower.contains("liberica") || lower.contains("bellsoft") {
        "BellSoft Liberica".to_string()
    } else if lower.contains("corretto") || lower.contains("amazon") {
        "Amazon Corretto".to_string()
    } else if lower.contains("jetbrains") {
        "JetBrains Runtime".to_string()
    } else if lower.contains("graalvm") {
        "GraalVM".to_string()
    } else if lower.contains("semeru") || lower.contains("ibm") {
        "IBM Semeru".to_string()
    } else if lower.contains("sapmachine") {
        "SapMachine".to_string()
    } else if lower.contains("java(tm)") || lower.contains("oracle") {
        "Oracle".to_string()
    } else if lower.contains("openjdk") {
        "OpenJDK".to_string()
    } else {
        "Java".to_string()
    }
}

/// Minimum required Java major version for a given Minecraft version string.
pub fn required_java_major(mc_version: &str) -> u32 {
    let v = mc_version.trim();

    // Handle snapshot versions (e.g. "24w14a", "25w02a")
    if let Some((year_str, rest)) = v.split_once('w')
        && year_str.len() == 2
        && let Ok(year) = year_str.parse::<u32>()
    {
        let week: u32 = rest
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse()
            .unwrap_or(0);
        if year > 24 || (year == 24 && week >= 14) {
            return 21;
        } else if year > 21 || (year == 21 && week >= 19) {
            return 17;
        } else if year == 21 {
            return 16;
        } else {
            return 8;
        }
    }

    let core = v.split(['-', '+', ' ']).next().unwrap_or(v);
    let mut parts = core.split('.');
    let major: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(1);
    let minor: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let patch: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);

    // Year-numbered releases (26.1, 26.3, also "26.1-snapshot-1") are built for Java 25.
    if major >= 26 {
        return 25;
    }
    if major >= 2 {
        return 21;
    }
    if minor > 20 || (minor == 20 && patch >= 5) {
        21
    } else if (18..=20).contains(&minor) {
        17
    } else if minor == 17 {
        16
    } else {
        8
    }
}

/// Pick the best Java installation for `mc_version`:
/// prefers an exact match for `required_java_major(mc_version)`, otherwise the
/// lowest installed major version >= required (with the newest patch version).
pub fn pick_best_java<'a>(
    installations: &'a [JavaInstallation],
    mc_version: &str,
) -> Option<&'a JavaInstallation> {
    let required = required_java_major(mc_version);

    let exact = installations
        .iter()
        .filter(|j| j.major == required)
        .max_by(|a, b| a.version.cmp(&b.version));
    if exact.is_some() {
        return exact;
    }

    installations
        .iter()
        .filter(|j| j.major > required)
        .min_by(|a, b| {
            a.major
                .cmp(&b.major)
                .then_with(|| b.version.cmp(&a.version))
        })
}

/// Resolve the Java executable path for a profile + settings, auto-detecting if configured to `"java"`.
pub fn resolve_java_for_mc(
    profile_java_path: Option<&str>,
    default_java_path: &str,
    mc_version: &str,
) -> String {
    if let Some(p) = profile_java_path {
        let trimmed = p.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    let def_trimmed = default_java_path.trim();
    if !def_trimmed.is_empty() && def_trimmed != "java" {
        return def_trimmed.to_string();
    }

    let installs = detect_java_installations();
    if let Some(best) = pick_best_java(&installs, mc_version) {
        return best.path.to_string_lossy().into_owned();
    }
    "java".to_string()
}
