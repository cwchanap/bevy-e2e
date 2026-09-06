use bevy::{
    ecs::entity::Entity,
    reflect::TypePath,
    remote::builtin_methods::{
        BRP_QUERY_METHOD, BrpQuery, BrpQueryFilter, BrpQueryParams, BrpQueryRow,
    },
};

use crate::{E2eId, Error, Result, game::Game};

impl Game {
    /// Returns whether exactly one entity currently carries `E2eId { value: id }`.
    ///
    /// Zero matches → `Ok(false)`. Two or more → `Err(AmbiguousSelector)`.
    pub fn exists(&self, id: &str) -> Result<bool> {
        match self.count_matches(id)? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(Error::AmbiguousSelector(id.to_owned())),
        }
    }

    /// Asserts that exactly one entity matches `id`.
    pub fn find(&self, id: &str) -> Result<()> {
        self.resolve_entity(id).map(|_| ())
    }

    /// Resolve `id` to a fresh raw `Entity` for this BRP call only (not cached).
    pub(crate) fn resolve_entity(&self, id: &str) -> Result<Entity> {
        let matches = self.matching_entities(id)?;
        match matches.len() {
            0 => Err(Error::SelectorNotFound(id.to_owned())),
            1 => Ok(matches[0]),
            _ => Err(Error::AmbiguousSelector(id.to_owned())),
        }
    }

    fn count_matches(&self, id: &str) -> Result<usize> {
        Ok(self.matching_entities(id)?.len())
    }

    fn matching_entities(&self, id: &str) -> Result<Vec<Entity>> {
        let type_path = E2eId::type_path();
        let params = BrpQueryParams {
            data: BrpQuery {
                components: vec![type_path.to_owned()],
                option: Default::default(),
                has: Vec::new(),
            },
            filter: BrpQueryFilter {
                with: vec![type_path.to_owned()],
                without: Vec::new(),
            },
            strict: true,
        };
        let params = serde_json::to_value(params).map_err(|error| Error::Brp {
            method: BRP_QUERY_METHOD.to_owned(),
            message: format!("failed to serialize world.query params: {error}"),
        })?;

        let result = self.brp(BRP_QUERY_METHOD, params)?;
        let rows: Vec<BrpQueryRow> =
            serde_json::from_value(result).map_err(|error| Error::Brp {
                method: BRP_QUERY_METHOD.to_owned(),
                message: format!("malformed world.query response: {error}"),
            })?;

        let mut matches = Vec::new();
        for row in rows {
            let Some(component) = row.components.get(type_path) else {
                continue;
            };
            let Some(value) = component.get("value").and_then(|v| v.as_str()) else {
                continue;
            };
            if value == id {
                matches.push(row.entity);
            }
        }
        Ok(matches)
    }
}
