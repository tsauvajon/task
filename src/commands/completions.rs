use std::io::{self, Write};

use crate::{
    commands::CompletionShell,
    error::{Error, Result},
};

/// Returns the shell completion script for the given shell.
pub(crate) fn script_for(shell: CompletionShell) -> &'static str {
    match shell {
        CompletionShell::Bash => {
            r#"_task_complete() {
    local IFS=$'\n'
    COMPREPLY=($(task __complete "${COMP_WORDS[@]:1}" 2>/dev/null))
}

complete -o nosort -F _task_complete task"#
        }
        CompletionShell::Fish => {
            r#"function __task_complete
    task __complete (commandline -opc | string split ' ' | tail -n +2) 2>/dev/null
end

complete -c task -f -a '(__task_complete)'"#
        }
        CompletionShell::Zsh => {
            r#"#compdef task

_task_complete() {
  local -a suggestions
  suggestions=("${(@f)$(task __complete "${words[@]:1}" 2>/dev/null)}")
  _describe 'values' suggestions
}

compdef _task_complete task"#
        }
    }
}

pub fn run(shell: CompletionShell) -> Result<()> {
    let script = script_for(shell);
    writeln!(io::stdout(), "{script}")
        .map_err(|err| Error::failed(format!("failed to write completions: {err}")))
}

#[cfg(test)]
mod tests {
    use super::script_for;
    use crate::commands::CompletionShell;

    mod script_for {
        use super::*;

        #[test]
        fn bash_registers_completion_function() {
            let script = script_for(CompletionShell::Bash);
            assert!(script.contains("_task_complete"), "missing function name");
            assert!(
                script.contains("complete -o nosort -F _task_complete task"),
                "missing complete directive"
            );
            assert!(
                script.contains("task __complete"),
                "missing task __complete invocation"
            );
        }

        #[test]
        fn fish_defines_helper_function() {
            let script = script_for(CompletionShell::Fish);
            assert!(
                script.contains("function __task_complete"),
                "missing function definition"
            );
            assert!(
                script.contains("complete -c task"),
                "missing complete directive"
            );
            assert!(
                script.contains("task __complete"),
                "missing task __complete invocation"
            );
        }

        #[test]
        fn zsh_has_compdef_header() {
            let script = script_for(CompletionShell::Zsh);
            assert!(
                script.starts_with("#compdef task"),
                "missing #compdef header"
            );
            assert!(
                script.contains("_task_complete"),
                "missing completion function"
            );
            assert!(
                script.contains("task __complete"),
                "missing task __complete invocation"
            );
        }

        #[test]
        fn shells_produce_distinct_scripts() {
            let bash = script_for(CompletionShell::Bash);
            let fish = script_for(CompletionShell::Fish);
            let zsh = script_for(CompletionShell::Zsh);
            assert_ne!(bash, fish);
            assert_ne!(bash, zsh);
            assert_ne!(fish, zsh);
        }
    }
}
