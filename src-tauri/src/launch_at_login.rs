use std::{fs, io::Write, path::Path};

const LABEL: &str = "com.llmwiki.desktop";

pub fn configure(home: &Path, executable: &Path, enabled: bool) -> Result<(), String> {
    let directory = home.join("Library/LaunchAgents");
    let path = directory.join(format!("{LABEL}.plist"));
    if !enabled {
        if path.exists() {
            fs::remove_file(path).map_err(|error| format!("disable launch at login: {error}"))?;
        }
        return Ok(());
    }
    fs::create_dir_all(&directory)
        .map_err(|error| format!("create LaunchAgents directory: {error}"))?;
    let executable = xml_escape(&executable.to_string_lossy());
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Label</key><string>{LABEL}</string>
  <key>ProgramArguments</key><array><string>{executable}</string></array>
  <key>RunAtLoad</key><true/>
</dict></plist>
"#
    );
    let temporary = path.with_extension("plist.tmp");
    let mut file =
        fs::File::create(&temporary).map_err(|error| format!("create LaunchAgent: {error}"))?;
    file.write_all(plist.as_bytes())
        .map_err(|error| format!("write LaunchAgent: {error}"))?;
    file.sync_all()
        .map_err(|error| format!("sync LaunchAgent: {error}"))?;
    fs::rename(temporary, path).map_err(|error| format!("replace LaunchAgent: {error}"))
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::xml_escape;

    #[test]
    fn escapes_launch_agent_values() {
        assert_eq!(xml_escape("a&<b>"), "a&amp;&lt;b&gt;");
    }
}
