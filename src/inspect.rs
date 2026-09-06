use bevy::remote::builtin_methods::{
    BRP_GET_COMPONENTS_METHOD, BRP_GET_RESOURCE_METHOD, BrpGetComponentsParams,
    BrpGetResourcesParams, BrpGetResourcesResponse,
};
use serde_json::{Map, Value};

use crate::{Error, Result, game::Game};

impl Game {
    /// Read one reflected component as JSON from the entity matching `id`.
    pub fn component_json(&self, id: &str, type_path: &str) -> Result<Value> {
        let entity = self.resolve_entity(id)?;
        let params = BrpGetComponentsParams {
            entity,
            components: vec![type_path.to_owned()],
            strict: true,
        };
        let params = serde_json::to_value(params).map_err(|error| Error::Brp {
            method: BRP_GET_COMPONENTS_METHOD.to_owned(),
            message: format!("failed to serialize world.get_components params: {error}"),
        })?;

        let result = self.brp(BRP_GET_COMPONENTS_METHOD, params)?;
        let components: Map<String, Value> =
            serde_json::from_value(result).map_err(|error| Error::Brp {
                method: BRP_GET_COMPONENTS_METHOD.to_owned(),
                message: format!("malformed world.get_components response: {error}"),
            })?;

        components
            .get(type_path)
            .cloned()
            .ok_or_else(|| Error::Brp {
                method: BRP_GET_COMPONENTS_METHOD.to_owned(),
                message: format!("strict get_components response missing `{type_path}`"),
            })
    }

    /// Read one reflected resource as JSON.
    pub fn resource_json(&self, type_path: &str) -> Result<Value> {
        let params = BrpGetResourcesParams {
            resource: type_path.to_owned(),
        };
        let params = serde_json::to_value(params).map_err(|error| Error::Brp {
            method: BRP_GET_RESOURCE_METHOD.to_owned(),
            message: format!("failed to serialize world.get_resources params: {error}"),
        })?;

        let result = self.brp(BRP_GET_RESOURCE_METHOD, params)?;
        let response: BrpGetResourcesResponse =
            serde_json::from_value(result).map_err(|error| Error::Brp {
                method: BRP_GET_RESOURCE_METHOD.to_owned(),
                message: format!("malformed world.get_resources response: {error}"),
            })?;
        Ok(response.value)
    }
}
