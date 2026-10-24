// ---
// relationships:
//   implements: command-line-interface
// ---
pub use toha::resolve::{
    ResolveError, ResolvedTemplate, applied_revision, formal_name, resolve_template,
};
pub fn load_context(
    dirs: &crate::Dirs,
    cwd: &std::path::Path,
) -> Result<(toha::config::Config, toha::registry::Registry), ResolveError> {
    let (config, registry, warnings) = toha::resolve::load_context(dirs, cwd)?;
    for warning in warnings {
        eprintln!("warning: {warning}");
    }
    Ok((config, registry))
}
