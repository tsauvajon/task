use crate::{error::Result, runtime::environment::RuntimeEnvironment};

pub fn run(context: &RuntimeEnvironment, repo_arg: Option<&str>) -> Result<()> {
    crate::ui::run(context, repo_arg)
}
