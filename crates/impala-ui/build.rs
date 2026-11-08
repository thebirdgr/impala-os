fn main() {
    // The ListView in the library uses std-widgets; pick a dark style that
    // ships with every Slint build (no Qt needed).
    let config = slint_build::CompilerConfiguration::new().with_style("fluent-dark".into());
    slint_build::compile_with_config("../../ui/main.slint", config).expect("compile ui/main.slint");
}
