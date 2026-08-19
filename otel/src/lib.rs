use phper::{
    modules::Module,
    php_get_module,
};
use std::env;

#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

pub mod context;
pub mod trace;
pub mod class_registry;
pub mod config;
pub mod error;
pub mod globals;
pub mod request;
pub mod logging;
pub mod logs;
pub mod runtime;
pub mod util;
pub mod module;
pub mod auto;

include!(concat!(env!("OUT_DIR"), "/package_versions.rs"));

/// Global allocator compiled into this build, reported via phpinfo().
pub const ALLOCATOR_NAME: &str = if cfg!(feature = "mimalloc") {
    "mimalloc"
} else {
    "system"
};

#[php_get_module]
pub fn get_module() -> Module {
    let mut module = Module::new(
        env!("CARGO_CRATE_NAME"),
        env!("CARGO_PKG_VERSION"),
        env!("CARGO_PKG_AUTHORS"),
    );

    module::add_module_info(&mut module);
    module::add_module_ini(&mut module);

    class_registry::register_classes_and_interfaces(&mut module);

    module.on_module_init(module::on_module_init);
    module.on_module_shutdown(module::on_module_shutdown);
    module.on_request_init(request::on_request_init);
    module.on_request_shutdown(request::on_request_shutdown);

    module
}
