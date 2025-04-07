#[cfg(feature = "log")]
pub mod rust_log {
    use crate::logger::Field;
    use std::panic::Location;

    pub fn log(
        level: log::Level,
        target: &str,
        module_path: &'static str,
        loc: &'static Location,
        kv_fields: Vec<Field>,
    ) {
        let kvs: Vec<(&str, log::kv::Value)> = kv_fields
            .iter()
            .filter_map(|field| match field {
                Field::KV(k, v) => match v {
                    Some(v) => Some((k.as_str(), log::kv::Value::from_display(v))),
                    None => Some((k.as_str(), log::kv::Value::null())),
                },
                _ => None,
            })
            .collect();
        let kvs = kvs.as_slice();

        let mut builder = log::Record::builder();

        builder
            .args(format_args!("access log"))
            .level(level)
            .target(target)
            .module_path_static(Some(module_path))
            .file_static(Some(loc.file()))
            .line(Some(loc.line()))
            .key_values(&kvs);

        log::logger().log(&builder.build());
    }
}
