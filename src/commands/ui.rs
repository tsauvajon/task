use crate::runtime::environment::RuntimeEnvironment;

pub fn run(context: &RuntimeEnvironment, repo_arg: Option<&str>) -> Result<(), String> {
    crate::ui::run(context, repo_arg)
}
