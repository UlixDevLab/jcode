use super::*;

#[test]
fn service_api_git_and_deployment_commands_do_not_require_native_consent() {
    for command in [
        "git commit -am 'ship fix'",
        "git push origin main",
        "gh pr create --fill",
        "stripe customers list",
        "vercel deploy --prod",
        "npx -y vercel --prod",
        "curl -H 'X-API-Key: literal-secret' https://api.example.test",
        "curl -u user:secret https://api.example.test",
        "curl https://api.stripe.com/v1/customers",
        "curl https://api.vercel.com/v6/deployments",
    ] {
        assert!(native_consent_requirement(command).is_none(), "{command}");
    }
}

#[test]
fn classifies_direct_browser_and_desktop_operations_without_local_false_positives() {
    for command in [
        "open https://example.test",
        "open -a Arc",
        "open /Applications/Arc.app",
        "sudo open https://example.test",
        "command open -a Safari",
        "builtin open -a Firefox",
        "sudo screencapture /tmp/desktop.png",
        "open Chrome",
        "open Safari",
        "open Firefox",
        "open Edge",
        "open Brave",
        "open /tmp/report.txt",
        "osascript -e 'tell application \"Safari\" to activate'",
        "osascript -e 'return 1 + 1'",
        "printf hello | pbcopy",
        "cliclick c:10,10",
        "screencapture /tmp/desktop.png",
    ] {
        assert!(native_consent_requirement(command).is_some(), "{command}");
    }
    for command in ["python tests/test_screencapture.py", "sudo ls -la"] {
        assert!(native_consent_requirement(command).is_none(), "{command}");
    }
}
