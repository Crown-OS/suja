const DESTRUCTIVE_COMMANDS: &[&str] = &[
    "rm",
    "rmdir",
    "mv",
    "dd",
    "mkfs",
    "shred",
    "truncate",
    "chmod",
    "chown",
    "sudo",
    "doas",
    "kill",
    "pkill",
    "killall",
    "reboot",
    "poweroff",
    "shutdown",
    "systemctl",
    "pacman",
    "git",
];
const DESTRUCTIVE_GIT: &[&str] = &[
    "reset", "clean", "push", "rebase", "checkout", "restore", "branch",
];
const REDIRECTS: &[&str] = &[">", ">>", "|", "&&", ";", "$(", "`"];

pub fn is_destructive(program: &str, args: &[String]) -> bool {
    let program = program.rsplit('/').next().unwrap_or(program);
    if program == "git" {
        return args
            .first()
            .is_some_and(|sub| DESTRUCTIVE_GIT.contains(&sub.as_str()));
    }
    if program == "systemctl" {
        return !args
            .iter()
            .any(|a| matches!(a.as_str(), "status" | "list-units" | "is-active" | "show"));
    }
    DESTRUCTIVE_COMMANDS.contains(&program)
        || args.iter().any(|a| REDIRECTS.iter().any(|r| a.contains(r)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn read_only_commands_pass() {
        assert!(!is_destructive("ls", &args(&["-la"])));
        assert!(!is_destructive("git", &args(&["status"])));
        assert!(!is_destructive(
            "systemctl",
            &args(&["--user", "status", "pipewire"])
        ));
    }

    #[test]
    fn destructive_commands_need_confirmation() {
        assert!(is_destructive("rm", &args(&["-rf", "x"])));
        assert!(is_destructive("/usr/bin/sudo", &args(&["ls"])));
        assert!(is_destructive("git", &args(&["push"])));
        assert!(is_destructive("echo", &args(&["hi", ">", "file"])));
    }
}
