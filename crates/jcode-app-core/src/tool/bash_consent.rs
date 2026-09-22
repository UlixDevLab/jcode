use jcode_tool_core::NativeConsentRequirement;

pub(super) fn native_consent_requirement(command: &str) -> Option<NativeConsentRequirement> {
    // Native consent is reserved for operations that touch the operator's
    // live desktop or foreground browser. Service/API/CLI work, including
    // authenticated network calls and git push, must not depend on a popup
    // that may be unavailable in a detached or remote session.
    direct_browser_or_desktop_requirement(command)
}

fn direct_browser_or_desktop_requirement(command: &str) -> Option<NativeConsentRequirement> {
    for segment in command.split(['|', ';', '&', '\n']) {
        let words = shell_words(segment);
        let Some(program) = executable(&words) else {
            continue;
        };
        let program = program.rsplit('/').next().unwrap_or(program);
        if program.eq_ignore_ascii_case("screencapture") {
            return Some(NativeConsentRequirement::new(
                "bash",
                "desktop capture",
                "live desktop",
            ));
        }
        if program.eq_ignore_ascii_case("open") {
            return Some(NativeConsentRequirement::new(
                "bash",
                "open external target",
                "desktop application",
            ));
        }
        if program.eq_ignore_ascii_case("osascript") || program.eq_ignore_ascii_case("cliclick") {
            return Some(NativeConsentRequirement::new(
                "bash",
                "desktop automation",
                "live desktop",
            ));
        }
        if program.eq_ignore_ascii_case("pbcopy") {
            return Some(NativeConsentRequirement::new(
                "bash",
                "clipboard write",
                "system clipboard",
            ));
        }
    }
    None
}

fn shell_words(segment: &str) -> Vec<&str> {
    segment.split_whitespace().map(trim_shell_quotes).collect()
}

fn executable<'a>(words: &'a [&'a str]) -> Option<&'a str> {
    let mut index = 0;
    while let Some(word) = words.get(index).copied() {
        if is_env_assignment(word) {
            index += 1;
            continue;
        }
        if word == "env" {
            index += 1;
            while let Some(argument) = words.get(index).copied() {
                if is_env_assignment(argument) || argument.starts_with('-') {
                    index += 1;
                } else {
                    break;
                }
            }
            continue;
        }
        if matches!(word, "sudo" | "command" | "builtin") {
            index += 1;
            while words
                .get(index)
                .is_some_and(|argument| argument.starts_with('-'))
            {
                index += 1;
            }
            continue;
        }
        return Some(word);
    }
    None
}

fn trim_shell_quotes(word: &str) -> &str {
    word.trim_matches(|character| matches!(character, '\'' | '"'))
}

fn is_env_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(key, _)| {
        !key.is_empty()
            && key
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '_')
    })
}

#[cfg(test)]
#[path = "bash_consent_tests.rs"]
mod tests;
