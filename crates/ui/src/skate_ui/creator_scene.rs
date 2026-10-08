use bevy::prelude::*;
use serde::Deserialize;
use serde_json::Value;
use skate_data::attrib_hash::numeric_name;
use std::{collections::BTreeMap, path::Path};

#[derive(Deserialize)]
struct Catalogue {
    version: u32,
    collections: Vec<Collection>,
}

#[derive(Deserialize)]
struct Collection {
    #[serde(rename = "class")]
    class_name: String,
    key: String,
    parent: String,
    fields: BTreeMap<String, Value>,
}

impl Catalogue {
    fn field(&self, class: &str, key: &str, name: &str) -> Result<&Value, String> {
        let class = numeric_name(class);
        let name = numeric_name(name);
        let mut key = numeric_name(key);
        for _ in 0..=self.collections.len() {
            let row = self
                .collections
                .iter()
                .find(|row| numeric_name(&row.class_name) == class && numeric_name(&row.key) == key)
                .ok_or("Missing creator scene collection")?;
            if let Some((_, field)) = row.fields.iter().find(|(key, _)| numeric_name(key) == name) {
                return Ok(field);
            }
            if row.parent.is_empty() {
                return Err("Missing creator scene field".into());
            }
            key = numeric_name(&row.parent);
        }
        Err("Cyclic creator scene inheritance".into())
    }

    fn numbers<const N: usize>(
        &self,
        class: &str,
        name: &str,
        kind: &str,
    ) -> Result<[f32; N], String> {
        let field = self.field(class, "default", name)?;
        if field.get("type").and_then(Value::as_str) != Some(kind) {
            return Err("Invalid creator scene field type".into());
        }
        let data = field
            .get("data")
            .and_then(Value::as_str)
            .ok_or("Missing creator scene field data")?;
        if !data.is_ascii() || data.len() != N * 8 {
            return Err("Invalid creator scene field length".into());
        }
        let mut values = [0.; N];
        for (i, value) in values.iter_mut().enumerate() {
            *value = f32::from_bits(
                u32::from_str_radix(&data[i * 8..i * 8 + 8], 16)
                    .map_err(|_| "Invalid creator scene field encoding")?,
            );
            if !value.is_finite() {
                return Err("Nonfinite creator scene value".into());
            }
        }
        Ok(values)
    }

    fn vector(&self, class: &str, name: &str) -> Result<Vec3, String> {
        let values = self.numbers::<4>(class, name, "Math::Vector3")?;
        Ok(Vec3::new(values[0], values[1], values[2]))
    }

    fn scalar(&self, class: &str, name: &str) -> Result<f32, String> {
        Ok(self.numbers::<1>(class, name, "EA::Reflection::Float")?[0])
    }
}

pub(super) struct CreatorScene {
    position: Vec3,
    aim: Vec3,
    up: Vec3,
    lens: f32,
    origin: Vec3,
    axis: Vec3,
    heading: f32,
}

impl CreatorScene {
    pub(super) fn load(root: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(root.join("private/customisation/native.json"))
            .map_err(|e| e.to_string())?;
        let catalogue: Catalogue = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if catalogue.version != 1 {
            return Err("Unsupported creator scene catalogue".into());
        }
        let scene = Self {
            position: catalogue.vector("cac_camera", "Hash_F323B5D55CA55A89")?,
            aim: catalogue.vector("cac_camera", "Hash_F0FC2B540F11109E")?,
            up: catalogue.vector("cac_camera", "Hash_513070D9CBBE801A")?,
            lens: catalogue.scalar("cac_camera", "Hash_EFB7F7EF35801C19")?,
            origin: catalogue.vector("cac_body", "Hash_6D2BEA1EFEA33425")?,
            axis: catalogue.vector("cac_body", "Hash_B0BBDA4D1028889D")?,
            heading: catalogue
                .scalar("cac_body", "Hash_4591287983616615")?
                .to_radians(),
        };
        if !(0.01..180.).contains(&scene.lens)
            || scene.axis.length_squared() < 1e-8
            || (scene.aim - scene.position).length_squared() < 1e-8
            || (scene.aim - scene.position)
                .cross(scene.up)
                .length_squared()
                < 1e-8
        {
            return Err("Degenerate creator scene camera or transform".into());
        }
        Ok(scene)
    }

    pub(super) fn clip_from_model(&self, aspect: f32, rotation: f32) -> Mat4 {
        let angle = self.lens.to_radians() * std::f32::consts::FRAC_2_PI;
        let projection = Mat4::from_cols(
            Vec4::new(1. / (angle * aspect), 0., 0., 0.),
            Vec4::new(0., 1. / angle, 0., 0.),
            Vec4::new(0., 0., 0., -1.),
            Vec4::new(0., 0., 0.05, 0.),
        );
        projection
            * Mat4::look_at_rh(self.position, self.aim, self.up)
            * Mat4::from_rotation_translation(
                Quat::from_axis_angle(self.axis.normalize(), self.heading + rotation),
                self.origin,
            )
    }
}
