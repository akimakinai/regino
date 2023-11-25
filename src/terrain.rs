use bevy::prelude::*;
use bevy::render::primitives::Aabb;
use bevy::{gltf::Gltf, scene::SceneInstanceReady};
use bevy_debug_text_overlay::screen_print;
use bevy_gltf_components::{ComponentsFromGltfPlugin, GltfLoadingTracker};
use bevy_xpbd_3d::components::Collider;
use bevy_xpbd_3d::prelude::*;

use crate::player;

pub struct TerrainPlugin;

impl Plugin for TerrainPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<MakeCollider>()
            .register_type::<SpawnPoint>()
            .register_type::<EnableShadow>()
            .register_type::<MakeLadder>()
            .add_plugins(ComponentsFromGltfPlugin)
            .add_systems(Startup, load_scene)
            .add_systems(Startup, load_scene)
            .add_systems(
                Update,
                spawn_scene.run_if(resource_changed::<GltfLoadingTracker>()),
            )
            .add_systems(
                Update,
                (
                    (
                        apply_enable_shadow::<PointLight>,
                        apply_enable_shadow::<SpotLight>,
                        make_collider,
                        spawn_point,
                    ),
                    show_scene,
                )
                    .chain(),
            )
            .add_systems(Update, make_collider)
            .add_systems(Update, make_ladder);
    }
}

#[derive(Resource)]
struct LevelGltf(Handle<Gltf>);

fn load_scene(mut commands: Commands, asset_server: Res<AssetServer>) {
    // Need to store `Handle<Gltf>` rather than `Handle<Scene>` because
    // gltf is dropped after spawning a scene directly.
    commands.insert_resource(LevelGltf(asset_server.load("levels/level.glb")));
}

fn show_scene(mut commands: Commands, mut ready_ev: EventReader<SceneInstanceReady>) {
    for ev in ready_ev.read() {
        let scene_root = ev.parent;
        commands.entity(scene_root).insert(Visibility::Visible);
    }
}

fn spawn_scene(
    mut commands: Commands,
    // mut asset_ev: EventReader<AssetEvent<Gltf>>,
    level_scene: Option<Res<LevelGltf>>,
    gltf: Res<Assets<Gltf>>,
    tracker: Res<GltfLoadingTracker>,
    mut done: Local<bool>,
) {
    debug_assert!(tracker.is_changed(), "enforced by run_if");

    if *done {
        return;
    }

    let Some(scene_handle) = level_scene else {
        return;
    };

    if !tracker.loaded_gltfs.contains(&scene_handle.0) {
        return;
    }

    *done = true;

    commands.spawn(SceneBundle {
        scene: gltf.get(scene_handle.0.clone()).unwrap().scenes[0].clone(),
        visibility: Visibility::Hidden,
        ..default()
    });
}

#[derive(Default, Component, Reflect)]
#[reflect(Component)]
struct SpawnPoint(bool);

fn spawn_point(
    mut commands: Commands,
    spawn_point: Query<(Entity, &SpawnPoint, &Children), Added<SpawnPoint>>,
    child: Query<(&Handle<Mesh>, &GlobalTransform)>,
    meshes: Res<Assets<Mesh>>,
) {
    for (entity, spawn_point, children) in &spawn_point {
        if !spawn_point.0 {
            continue;
        }

        screen_print!("spawning point at {:?}", entity);

        commands.entity(entity).despawn();

        let Ok((mesh, gtransform)) = child.get(children[0]) else {
            continue;
        };

        let mesh = meshes.get(mesh).unwrap();

        commands
            .spawn(SpatialBundle::from_transform(Transform::from_translation(
                gtransform.translation() + Vec3::from(mesh.compute_aabb().unwrap().center),
            )))
            .insert(player::Player);
    }
}

#[derive(Default, Component, Reflect)]
#[reflect(Component)]
struct EnableShadow(bool);

trait ApplyEnableShadow: Component {
    fn enable_shadow(&mut self);
}

impl ApplyEnableShadow for PointLight {
    fn enable_shadow(&mut self) {
        self.shadows_enabled = true;
    }
}

impl ApplyEnableShadow for SpotLight {
    fn enable_shadow(&mut self) {
        self.shadows_enabled = true;
    }
}

fn apply_enable_shadow<T: ApplyEnableShadow>(
    mut commands: Commands,
    mut lights: Query<(Entity, &EnableShadow, &mut T)>,
) {
    for (entity, enable_shadow, mut light) in &mut lights {
        commands.entity(entity).remove::<EnableShadow>();

        if !enable_shadow.0 {
            continue;
        }

        light.enable_shadow();
    }
}

#[derive(Default, Component, Reflect)]
#[reflect(Component)]
struct MakeCollider(bool);

fn make_collider(
    target: Query<(Entity, &MakeCollider, &Transform, &Children), Added<MakeCollider>>,
    mesh: Query<&Handle<Mesh>>,
    meshes: Res<Assets<Mesh>>,
    mut commands: Commands,
) {
    for (entity, make_collider, transform, children) in target.iter() {
        if !make_collider.0 {
            continue;
        }

        screen_print!("making collider for entity {:?}", entity);

        let mesh = meshes.get(mesh.get(children[0]).unwrap()).unwrap();
        let Some(collider) = Collider::convex_hull_from_mesh(mesh) else {
            error!("Failed to create collider for entity {:?}", entity);
            continue;
        };

        commands
            .entity(entity)
            .insert((collider, RigidBody::Static))
            .insert(ColliderTransform {
                // Meshes are not scaled, so we need to scale the collider
                scale: transform.scale,
                ..default()
            });
    }
}

#[derive(Default, Component, Reflect)]
#[reflect(Component)]
struct MakeLadder(bool);

#[derive(Component)]
pub struct Ladder {
    pub face_normal: Vec3,
}

// bevy_gizmos::aabb_transform
fn aabb_transform(aabb: Aabb, transform: GlobalTransform) -> GlobalTransform {
    transform
        * GlobalTransform::from(
            Transform::from_translation(aabb.center.into())
                .with_scale((aabb.half_extents * 2.).into()),
        )
}

fn make_ladder(
    mut commands: Commands,
    query: Query<Entity, Added<MakeLadder>>,
    children: Query<&Children>,
    has_mesh: Query<(Entity, &Handle<Mesh>, &GlobalTransform)>,
    meshes: Res<Assets<Mesh>>,
) {
    for ladder_entity in query.iter() {
        if let Some((mesh_entity, mesh, gtransform)) = children
            .iter_descendants(ladder_entity)
            .find_map(|e| has_mesh.get(e).ok())
        {
            let aabb = meshes
                .get(mesh)
                .unwrap()
                .compute_aabb()
                .expect("Failed to compute AABB for ladder mesh");
            let half_extents = Vec3::from(aabb.half_extents) * gtransform.compute_transform().scale;

            let face_normal = Vec3::Z;
            screen_print!("half_extents: {:?}", half_extents);

            let position = Position(aabb_transform(aabb, *gtransform).translation());

            commands
                .entity(mesh_entity)
                // // This results in incorrect scaling:
                // .insert((
                //     AsyncCollider(ComputedCollider::ConvexHull),
                //     RigidBody::Static,
                // ));
                .with_children(|cmd| {
                    cmd.spawn((
                        Ladder {
                            face_normal,
                        },
                        Collider::cuboid(
                            half_extents.x * 2.0,
                            half_extents.y * 2.0,
                            half_extents.z * 2.0,
                        ),
                        RigidBody::Static,
                        position,
                    ));
                });
        }
    }
}
