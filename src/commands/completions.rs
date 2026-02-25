use std::io::{self, Write};

use crate::{
    commands::CompletionShell,
    error::{Error, Result},
};

pub fn run(shell: CompletionShell) -> Result<()> {
    let script = match shell {
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
    };

    writeln!(io::stdout(), "{script}")
        .map_err(|err| Error::failed(format!("failed to write completions: {err}")))
}
