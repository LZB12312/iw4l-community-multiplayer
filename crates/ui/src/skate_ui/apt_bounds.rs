use super::{apt_movie::Movie, apt_vm::Vm};
use std::collections::BTreeMap;

pub type Bounds = BTreeMap<i32, [f64; 4]>;

pub fn authored(json: &serde_json::Value) -> Result<Bounds, String> {
    let mut out = Bounds::new();
    for (key, shapes) in json["shapes"].as_object().ok_or("Movie shapes missing")? {
        let mut rect = None;
        for shape in shapes.as_array().ok_or("Shape list missing")? {
            for triangle in shape["triangles"].as_array().ok_or("Triangles missing")? {
                for vertex in triangle.as_array().ok_or("Triangle missing")? {
                    let x = vertex["position"][0]
                        .as_f64()
                        .filter(|v| v.is_finite())
                        .ok_or("Invalid vertex x")?;
                    let y = vertex["position"][1]
                        .as_f64()
                        .filter(|v| v.is_finite())
                        .ok_or("Invalid vertex y")?;
                    include(&mut rect, [x, y, x, y]);
                }
            }
        }
        if let Some(rect) = rect {
            out.insert(key.parse().map_err(|_| "Invalid shape character")?, rect);
        }
    }
    Ok(out)
}

fn include(rect: &mut Option<[f64; 4]>, next: [f64; 4]) {
    *rect = Some(match *rect {
        None => next,
        Some(r) => [
            r[0].min(next[0]),
            r[1].min(next[1]),
            r[2].max(next[2]),
            r[3].max(next[3]),
        ],
    });
}

pub fn local(
    movie: &Movie,
    vm: &Vm,
    shapes: &Bounds,
    id: usize,
    depth: usize,
) -> Result<Option<[f64; 4]>, String> {
    if depth > 32 {
        return Err("Movie bounds nesting limit".into());
    }
    let instance = movie
        .instances
        .get(&id)
        .ok_or("Movie bounds instance missing")?;
    let character = &movie.characters[&instance.character];
    let mut rect = shapes
        .get(&instance.character)
        .copied()
        .or_else(|| character.bounds.map(|r| r.map(f64::from)));
    for child in instance.children.values() {
        let Some(r) = local(movie, vm, shapes, *child, depth + 1)? else {
            continue;
        };
        let placement = movie.instances[child].placement.as_ref();
        let mut matrix = placement.map_or([1., 0., 0., 1., 0., 0.], |p| p.matrix.map(f64::from));
        matrix[4] = vm.get(*child, "_x").number();
        matrix[5] = vm.get(*child, "_y").number();
        for point in [[r[0], r[1]], [r[2], r[1]], [r[2], r[3]], [r[0], r[3]]] {
            let x = matrix[0] * point[0] + matrix[2] * point[1] + matrix[4];
            let y = matrix[1] * point[0] + matrix[3] * point[1] + matrix[5];
            if !x.is_finite() || !y.is_finite() {
                return Err("Invalid movie bounds transform".into());
            }
            include(&mut rect, [x, y, x, y]);
        }
    }
    Ok(rect)
}
