#[cfg(any(target_os = "linux", test))]
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

#[cfg(any(target_os = "linux", test))]
const CLIENT_MAIN_CLASSES: &[&str] = &[
    "net.minecraft.client.main.Main",
    "cpw.mods.bootstraplauncher.BootstrapLauncher",
    "net.neoforged.bootstraplauncher.BootstrapLauncher",
    "net.neoforged.fml.loading.targets.CommonLaunchHandler",
];

pub fn canonical_instance_root(root: &Path) -> Result<PathBuf, String> {
    let root = fs::canonicalize(root)
        .map_err(|err| format!("Could not resolve the Mars game directory: {err}"))?;
    if !root.is_dir() {
        return Err("The configured Mars game directory is unavailable".into());
    }
    #[cfg(windows)]
    {
        let root = root.to_string_lossy();
        let root = if let Some(unc_path) = root.strip_prefix(r"\\?\UNC\") {
            format!(r"\\{unc_path}")
        } else {
            root.strip_prefix(r"\\?\").unwrap_or(&root).to_string()
        };
        return Ok(PathBuf::from(root));
    }
    #[cfg(not(windows))]
    Ok(root)
}

pub fn has_minecraft_client(instance_root: &Path) -> Result<bool, String> {
    #[cfg(target_os = "linux")]
    {
        return linux_has_minecraft_client(instance_root);
    }
    #[cfg(target_os = "windows")]
    {
        return windows_has_minecraft_client(instance_root);
    }
    #[allow(unreachable_code)]
    Err("Minecraft process detection is only supported on Windows and Linux.".into())
}

#[cfg(any(target_os = "linux", test))]
fn java_command_matches(args: &[OsString], instance_root: &Path, windows: bool) -> bool {
    let Some(executable) = args.first() else {
        return false;
    };
    let executable = executable.to_string_lossy();
    let executable_name = executable
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !matches!(
        executable_name.as_str(),
        "java" | "java.exe" | "javaw" | "javaw.exe"
    ) {
        return false;
    }

    let normalize = |path: &str| {
        let path = path.replace('/', "\\");
        if windows {
            path.to_lowercase()
        } else {
            path
        }
    };
    let expected_root = normalize(&instance_root.to_string_lossy());
    let mut game_dir_matches = false;
    for (index, argument) in args.iter().enumerate().skip(1) {
        let argument = argument.to_string_lossy();
        if argument == "--gameDir" {
            game_dir_matches = args
                .get(index + 1)
                .is_some_and(|value| normalize(&value.to_string_lossy()) == expected_root);
            break;
        }
        if let Some(value) = argument.strip_prefix("--gameDir=") {
            game_dir_matches = normalize(value) == expected_root;
            break;
        }
    }
    game_dir_matches
        && args
            .iter()
            .any(|argument| CLIENT_MAIN_CLASSES.contains(&argument.to_string_lossy().as_ref()))
}

#[cfg(target_os = "linux")]
fn linux_has_minecraft_client(instance_root: &Path) -> Result<bool, String> {
    let processes =
        fs::read_dir("/proc").map_err(|err| format!("Could not inspect Linux processes: {err}"))?;
    for process in processes.flatten() {
        if !process
            .file_name()
            .to_string_lossy()
            .bytes()
            .all(|byte| byte.is_ascii_digit())
        {
            continue;
        }
        let Ok(command_line) = fs::read(process.path().join("cmdline")) else {
            continue;
        };
        let args: Vec<OsString> = command_line
            .split(|byte| *byte == 0)
            .filter(|argument| !argument.is_empty())
            .map(|argument| OsString::from(String::from_utf8_lossy(argument).into_owned()))
            .collect();
        if java_command_matches(&args, instance_root, false) {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(target_os = "windows")]
fn windows_has_minecraft_client(instance_root: &Path) -> Result<bool, String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::thread;
    use std::time::{Duration, Instant};

    const QUERY: &str = r#"
$ErrorActionPreference = 'Stop'
$root = $env:MARS_INSTANCE_ROOT.Replace('/', '\').ToLowerInvariant()
$dirPattern = '(?:^|\s)"?--gameDir"?(?:\s+|=)"?' + [regex]::Escape($root) + '"?(?=\s|$)'
$classPattern = '(?:^|\s)"?(?:net\.minecraft\.client\.main\.main|cpw\.mods\.bootstraplauncher\.bootstraplauncher|net\.neoforged\.bootstraplauncher\.bootstraplauncher|net\.neoforged\.fml\.loading\.targets\.commonlaunchhandler)"?(?=\s|$)'
$found = Get-CimInstance -ClassName Win32_Process -Filter "Name = 'java.exe' OR Name = 'javaw.exe'" | Where-Object {
    $line = $_.CommandLine
    if ([string]::IsNullOrEmpty($line)) { $false }
    else {
        $line = $line.Replace('/', '\').ToLowerInvariant()
        ($line -match $dirPattern) -and ($line -match $classPattern)
    }
} | Select-Object -First 1
if ($null -eq $found) { 'false' } else { 'true' }
"#;

    let root = instance_root.to_string_lossy();
    let mut child = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", QUERY])
        .env("MARS_INSTANCE_ROOT", root.as_ref())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| format!("Could not query Windows Java processes: {err}"))?;
    let started_at = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut output = String::new();
                child
                    .stdout
                    .take()
                    .expect("PowerShell stdout is piped")
                    .read_to_string(&mut output)
                    .map_err(|err| format!("Could not read the process query result: {err}"))?;
                if !status.success() {
                    return Err("Windows Java process query did not complete successfully".into());
                }
                return match output.trim() {
                    "true" => Ok(true),
                    "false" => Ok(false),
                    _ => Err("Windows Java process query returned an invalid result".into()),
                };
            }
            Ok(None) if started_at.elapsed() < Duration::from_secs(5) => {
                thread::sleep(Duration::from_millis(50));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Windows Java process query timed out".into());
            }
            Err(err) => return Err(format!("Could not inspect Windows Java processes: {err}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::java_command_matches;
    use std::ffi::OsString;
    use std::path::Path;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn matches_only_java_client_process_for_the_configured_game_directory() {
        let root = Path::new("/home/player/.minecraft/mars-client");
        assert!(java_command_matches(
            &args(&[
                "/usr/bin/java",
                "-cp",
                "libraries/*",
                "net.minecraft.client.main.Main",
                "--gameDir",
                "/home/player/.minecraft/mars-client",
            ]),
            root,
            false,
        ));
        assert!(!java_command_matches(
            &args(&[
                "/usr/bin/java",
                "net.minecraft.client.main.Main",
                "--gameDir",
                "/home/player/.minecraft/other",
            ]),
            root,
            false,
        ));
        assert!(!java_command_matches(
            &args(&[
                "/usr/bin/java",
                "com.example.UnrelatedMain",
                "--gameDir",
                "/home/player/.minecraft/mars-client",
            ]),
            root,
            false,
        ));
        assert!(!java_command_matches(
            &args(&[
                "/usr/bin/not-java",
                "net.minecraft.client.main.Main",
                "--gameDir",
                "/home/player/.minecraft/mars-client",
            ]),
            root,
            false,
        ));
    }

    #[test]
    fn matches_neoforge_and_normalizes_windows_paths() {
        assert!(java_command_matches(
            &args(&[
                "C:\\Java\\javaw.exe",
                "net.neoforged.bootstraplauncher.BootstrapLauncher",
                "--gameDir=C:/Games/Mars",
            ]),
            Path::new("c:\\games\\mars"),
            true,
        ));
    }
}
