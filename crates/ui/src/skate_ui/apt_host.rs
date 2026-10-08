use super::{
    apt_movie::Movie,
    apt_vm::{Host, ObjectKind, Value, Vm},
};

pub trait MovieHost: Host {
    fn movie(&mut self) -> &mut Movie;
}

pub fn call<H: MovieHost>(
    host: &mut H,
    vm: &mut Vm,
    id: usize,
    method: &str,
    args: &[Value],
) -> Result<Option<Value>, String> {
    if host.movie().prepare_constructor(vm, id)? {
        vm.call_method(id, "constructor", vec![], host)?;
        return vm.call_method(id, method, args.to_vec(), host).map(Some);
    }
    if method == "attachMovie" && host.movie().instances.contains_key(&id) {
        let symbol = args.first().ok_or("Missing movie symbol")?.text();
        let name = args.get(1).ok_or("Missing attached movie name")?.text();
        let depth = args.get(2).ok_or("Missing attached movie depth")?.number();
        if !depth.is_finite()
            || depth.fract() != 0.
            || depth < i32::MIN as f64
            || depth > i32::MAX as f64
        {
            return Err("Invalid attached movie depth".into());
        }
        let init = match args.get(3) {
            Some(Value::Object(object)) => Some(*object),
            Some(Value::Undefined) | None => None,
            _ => return Err("Invalid attached movie parameters".into()),
        };
        let character = host.movie().export(id, &symbol)?;
        let attached = host
            .movie()
            .attach(vm, id, character, name, depth as i32, init)?;
        if host.movie().prepare_constructor(vm, attached)? {
            vm.call_method(attached, "constructor", vec![], host)?;
        }
        return Ok(Some(Value::Object(attached)));
    }
    if host.movie().method(vm, id, method, args)? {
        return Ok(Some(Value::Undefined));
    }
    if matches!(method, "push" | "pop")
        && let Value::Number(length) = vm.get(id, "length")
    {
        if !length.is_finite() || !(0. ..=4096.).contains(&length) || length.fract() != 0. {
            return Err("APT array size limit".into());
        }
        if method == "pop" {
            if length == 0. {
                return Ok(Some(Value::Undefined));
            }
            let key = (length as usize - 1).to_string();
            let value = vm.get(id, &key);
            vm.objects[id].fields.remove(&key);
            vm.set(id, "length", Value::Number(length - 1.))?;
            return Ok(Some(value));
        }
        if args.len() > 4096 - length as usize {
            return Err("APT array size limit".into());
        }
        for (index, value) in args.iter().enumerate() {
            vm.set(id, (length as usize + index).to_string(), value.clone())?;
        }
        let length = length + args.len() as f64;
        vm.set(id, "length", Value::Number(length))?;
        return Ok(Some(Value::Number(length)));
    }
    let native = match &vm.objects.get(id).ok_or("Missing UI object")?.kind {
        ObjectKind::Native(name) => name.as_str(),
        _ => "global",
    };
    let first = || args.first().ok_or("Missing UI argument");
    let value = match (native, method) {
        ("global", "ASSetPropFlags" | "trace") => Value::Undefined,
        ("MovieClip" | "Object", "registerClass") => {
            let symbol = first()?.text();
            let Some(Value::Object(class)) = args.get(1) else {
                return Err("Missing movie class".into());
            };
            host.movie().register_class(&symbol, *class)?;
            Value::Bool(true)
        }
        ("Math", "floor") => Value::Number(first()?.number().floor()),
        ("Math", "ceil") => Value::Number(first()?.number().ceil()),
        ("FELanguage", "GetLocaleText") => {
            Value::Text(host.movie().text_assets.localize(&first()?.text()))
        }
        _ => return Ok(None),
    };
    Ok(Some(value))
}

pub fn drain<H: MovieHost>(host: &mut H, vm: &mut Vm) -> Result<(), String> {
    for _ in 0..4096 {
        let constructors = host.movie().prepare_constructors(vm)?;
        for id in constructors {
            if host.movie().instances.contains_key(&id) {
                vm.call_method(id, "constructor", vec![], host)?;
            }
        }
        let Some((id, offset)) = host.movie().pending.pop_front() else {
            return Ok(());
        };
        if !host.movie().instances.contains_key(&id) {
            continue;
        }
        let code = host
            .movie()
            .actions
            .get(&offset.to_string())
            .ok_or("Missing UI action")?
            .clone();
        vm.run_on(id, &code, host)?;
    }
    Err("UI action limit".into())
}
