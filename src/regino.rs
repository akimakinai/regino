use bevy::app::{PluginGroup, PluginGroupBuilder};

use crate::terrain::TerrainPlugin;
use crate::player::PlayerPlugin;

pub struct ReginoPlugins;

impl PluginGroup for ReginoPlugins {
    fn build(self) -> PluginGroupBuilder {
        PluginGroupBuilder::start::<ReginoPlugins>()
            .add(TerrainPlugin)
            .add(PlayerPlugin)
    }
}
