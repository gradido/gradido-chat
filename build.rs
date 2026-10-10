fn main() {
    // The same look on every system; the native styles differ too much for one design.
    let config = slint_build::CompilerConfiguration::new().with_style("fluent".into());
    slint_build::compile_with_config("ui/app.slint", config).expect("the UI must compile");
}
