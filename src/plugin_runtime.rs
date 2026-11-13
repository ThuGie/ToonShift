use wasmi::{Config, Engine, Linker, Module, Store, StoreLimits, StoreLimitsBuilder};

pub fn decode(code: &str) -> Result<Vec<u8>, String> {
    if code.len() > 192000
        || !code.len().is_multiple_of(2)
        || !code.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("Invalid plugin code encoding".into());
    }
    (0..code.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&code[index..index + 2], 16).map_err(|e| e.to_string()))
        .collect()
}

pub fn render(code: &[u8], input: &[u8]) -> Result<String, String> {
    run(code, input, false)
}

pub fn render_feed(code: &[u8], input: &[u8]) -> Result<String, String> {
    run(code, input, true)
}

pub fn validate(code: &[u8], structured: bool) -> Result<(), String> {
    let (store, instance) = instantiate(code, structured)?;
    instance
        .get_memory(&store, "memory")
        .ok_or("Plugin must export memory")?;
    if structured {
        instance
            .get_typed_func::<(), i32>(&store, "input_buffer")
            .map_err(|e| e.to_string())?;
    }
    instance
        .get_typed_func::<i32, i64>(&store, "render")
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn run(code: &[u8], input: &[u8], structured: bool) -> Result<String, String> {
    let (mut store, instance) = instantiate(code, structured)?;
    let memory = instance
        .get_memory(&store, "memory")
        .ok_or("Plugin must export memory")?;
    if input.len() > if structured { 32770 } else { 32768 } {
        return Err("Plugin input exceeds 32 KiB".into());
    }
    let offset = if structured {
        instance
            .get_typed_func::<(), i32>(&store, "input_buffer")
            .and_then(|func| func.call(&mut store, ()))
            .map_err(|e| e.to_string())? as u32 as usize
    } else {
        0
    };
    memory
        .write(&mut store, offset, input)
        .map_err(|e| e.to_string())?;
    let render = instance
        .get_typed_func::<i32, i64>(&store, "render")
        .map_err(|e| e.to_string())?;
    let output = render
        .call(&mut store, input.len() as i32)
        .map_err(|e| e.to_string())? as u64;
    let len = (output & 0xffff_ffff) as usize;
    if !(1..=if structured { 4096 } else { 256 }).contains(&len) {
        return Err("Invalid plugin output length".into());
    }
    let mut text = vec![0; len];
    memory
        .read(&store, (output >> 32) as usize, &mut text)
        .map_err(|e| e.to_string())?;
    String::from_utf8(text).map_err(|e| e.to_string())
}

fn instantiate(
    code: &[u8],
    structured: bool,
) -> Result<(Store<StoreLimits>, wasmi::Instance), String> {
    let mut config = Config::default();
    config.consume_fuel(true);
    config.enforced_limits(wasmi::EnforcedLimits::strict());
    let engine = Engine::new(&config);
    let module = Module::new(&engine, code).map_err(|e| e.to_string())?;
    let limits = StoreLimitsBuilder::new()
        .memory_size(if structured { 512 * 1024 } else { 128 * 1024 })
        .table_elements(128)
        .instances(1)
        .memories(1)
        .tables(1)
        .build();
    let mut store = Store::new(&engine, limits);
    store.limiter(|limits| limits);
    store
        .set_fuel(if structured { 4_000_000 } else { 100_000 })
        .map_err(|e| e.to_string())?;
    let instance = Linker::<StoreLimits>::new(&engine)
        .instantiate(&mut store, &module)
        .and_then(|pre| pre.ensure_no_start(&mut store).map_err(Into::into))
        .map_err(|e| e.to_string())?;
    Ok((store, instance))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_code_and_rejects_unbounded_execution_and_imports() {
        let echo = wat::parse_str(
            r#"(module (memory (export "memory") 1)
            (func (export "render") (param i32) (result i64) local.get 0 i64.extend_i32_u))"#,
        )
        .unwrap();
        assert_eq!(render(&echo, b"Hello").unwrap(), "Hello");
        assert!(validate(&echo, false).is_ok());
        assert!(validate(&echo, true).is_err());
        let looping = wat::parse_str(
            r#"(module (memory (export "memory") 1)
            (func (export "render") (param i32) (result i64) (loop br 0) i64.const 0))"#,
        )
        .unwrap();
        assert!(render(&looping, b"").is_err());
        let imports =
            wat::parse_str(r#"(module (import "wasi_snapshot_preview1" "proc_exit" (func)))"#)
                .unwrap();
        assert!(render(&imports, b"").is_err());
        assert!(validate(&imports, false).is_err());
    }
}
