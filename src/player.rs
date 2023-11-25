use bevy::prelude::*;
use bevy_debug_text_overlay::screen_print;
use bevy_tnua::{builtins::TnuaBuiltinWalk, controller::TnuaController, TnuaUserControlsSystemSet};
use bevy_tnua::{control_helpers::TnuaCrouchEnforcerPlugin, prelude::*};
use bevy_tnua_xpbd3d::*;
use bevy_xpbd_3d::prelude::*;
use leafwing_input_manager::prelude::*;
use smooth_bevy_cameras::{LookTransform, LookTransformBundle, LookTransformPlugin, Smoother};

use crate::terrain::Ladder;

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, startup);
        build_player_add(app);
        build_movement(app);
        build_player_camera(app);

        app.add_systems(
            Update,
            (
                player_interaction,
                (added_player_walking, added_player_moving_on_ladder),
                apply_deferred,
            )
                .chain()
                .before(TnuaPipelineStages::Motors),
        );
    }
}

fn startup(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mut mats = vec![];

    for i in 0..=1 {
        let image = assets.load::<Image>(format!("sprites/cute_apple_run_{i}_cropped.png"));
        mats.push(materials.add(StandardMaterial {
            alpha_mode: AlphaMode::Mask(0.1),
            double_sided: true,
            cull_mode: None,
            ..image.into()
        }));
    }

    commands.insert_resource(PlayerImages(mats));
}

#[derive(Component, Debug)]
pub struct Player;

#[derive(Resource)]
struct PlayerImages(Vec<Handle<StandardMaterial>>);

fn build_player_add(app: &mut App) {
    app.add_systems(Update, add_player);
}

#[derive(Component)]
struct PlayerWalking;

fn added_player_walking(
    mut commands: Commands,
    mut player: Query<(Entity, &mut RigidBody), (Added<PlayerWalking>, Without<TnuaController>)>,
) {
    for (entity, mut rigid_body) in player.iter_mut() {
        rigid_body.set_if_neq(RigidBody::Dynamic);
        commands
            .entity(entity)
            .insert(TnuaControllerBundle::default());
    }
}

#[derive(Component)]
struct PlayerMovingOnLadder {
    ladder_normal: Vec3,
    ladder_top: Vec3,
    ladder_bottom: Vec3,
}

fn added_player_moving_on_ladder(
    mut commands: Commands,
    mut player: Query<
        (
            Entity,
            &mut RigidBody,
            &mut LinearVelocity,
            Has<TnuaController>,
        ),
        (With<Player>, Added<PlayerMovingOnLadder>),
    >,
) {
    for (entity, mut rigid_body, mut linvel, has_controller) in player.iter_mut() {
        rigid_body.set_if_neq(RigidBody::Kinematic);
        linvel.0 = Vec3::ZERO;
        if has_controller {
            commands.entity(entity).remove::<TnuaControllerBundle>();
        }
    }
}

#[derive(Component, PartialEq, Debug)]
enum PlayerActionState {
    Grounded,
    Jumping,
}

const PLAYER_HEIGHT: f32 = 1.0;
const PLAYER_WIDTH: f32 = 1.0;

#[derive(Component)]
struct InteractionRayCaster;

fn add_player(
    mut commands: Commands,
    player: Query<Entity, Added<Player>>,
    images: Res<PlayerImages>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    for entity in player.iter() {
        commands
            .entity(entity)
            .insert(Name::new("Player"))
            .insert((
                Collider::capsule(PLAYER_HEIGHT / 4., PLAYER_WIDTH / 4.),
                RigidBody::Dynamic,
            ))
            .insert(LockedAxes::new().lock_rotation_x().lock_rotation_z())
            .insert(TnuaControllerBundle::default())
            .insert((PlayerActionState::Grounded, PlayerWalking))
            .insert((
                meshes.add(Mesh::from(shape::Quad::new(Vec2::new(
                    PLAYER_WIDTH,
                    PLAYER_HEIGHT,
                )))),
                images.0[0].clone(),
                VisibilityBundle::default(),
            ))
            .with_children(|builder| {
                // RayCaster for interaction
                builder.spawn((
                    Name::new("InteractionRayCaster"),
                    InteractionRayCaster,
                    RayCaster::new(Vec3::ZERO, -Vec3::Z)
                        .with_max_time_of_impact(PLAYER_WIDTH * 0.8)
                        .with_query_filter(
                            SpatialQueryFilter::new().without_entities([builder.parent_entity()]),
                        ),
                    SpatialBundle::default(),
                ));
            });
    }
}

// Player movement

fn build_movement(app: &mut App) {
    app.add_plugins((
        TnuaXpbd3dPlugin,
        TnuaControllerPlugin,
        TnuaCrouchEnforcerPlugin,
    ))
    .add_plugins(InputManagerPlugin::<Action>::default())
    .add_systems(Update, add_action_state)
    .add_systems(
        FixedUpdate,
        (
            player_movement_action,
            player_movement_walk,
            player_movement_ladder,
        )
            .in_set(TnuaUserControlsSystemSet),
    )
    .add_systems(Update, player_animation);
}

#[derive(Actionlike, PartialEq, Eq, Clone, Copy, Hash, Debug, Reflect)]
enum Action {
    Up,
    Down,
    Left,
    Right,
    Jump,
    Interact,
}

fn add_action_state(
    mut commands: Commands,
    player: Query<Entity, (With<Player>, Without<ActionState<Action>>)>,
) {
    for entity in player.iter() {
        commands
            .entity(entity)
            .insert(InputManagerBundle::<Action> {
                action_state: default(),
                input_map: InputMap::new([
                    // WASD
                    (KeyCode::W, Action::Up),
                    (KeyCode::S, Action::Down),
                    (KeyCode::A, Action::Left),
                    (KeyCode::D, Action::Right),
                    // Cursor keys
                    (KeyCode::Up, Action::Up),
                    (KeyCode::Down, Action::Down),
                    (KeyCode::Left, Action::Left),
                    (KeyCode::Right, Action::Right),
                    // Space
                    (KeyCode::Space, Action::Jump),
                    // E
                    (KeyCode::E, Action::Interact),
                ]),
            });
    }
}

fn player_movement_action(
    mut player: Query<
        (
            &ActionState<Action>,
            &mut TnuaController,
            &mut PlayerActionState,
        ),
        With<Player>,
    >,
) {
    for (input, mut controller, mut action_state) in player.iter_mut() {
        let jumping = controller.concrete_action::<TnuaBuiltinJump>().is_some();
        if jumping && input.just_released(Action::Jump) {
            *action_state = PlayerActionState::Jumping;
        }
        if !jumping {
            *action_state = PlayerActionState::Grounded;
        }

        if *action_state == PlayerActionState::Grounded {
            if input.just_pressed(Action::Jump) || (jumping && input.pressed(Action::Jump)) {
                controller.action(TnuaBuiltinJump {
                    height: 1.0,
                    ..default()
                });
            }
        }
    }
}

fn insert_or_modify<T: Component>(
    commands: &mut Commands,
    entity: Entity,
    component: &mut Option<Mut<T>>,
    insert: impl Fn() -> T,
    modify: impl FnOnce(&mut T),
) {
    if let Some(mut c) = component.as_mut() {
        modify(&mut c);
    } else {
        let mut c = insert();
        modify(&mut c);
        commands.entity(entity).insert(c);
    }
}

fn player_movement_walk(
    mut commands: Commands,
    mut player: Query<
        (Entity, &ActionState<Action>, Option<&mut TnuaController>),
        (With<Player>, With<PlayerWalking>),
    >,
) {
    const MOVEMENT_SPEED: f32 = 2.0;

    for (entity, input, mut controller) in player.iter_mut() {
        let mut movement = Vec3::ZERO;

        if input.pressed(Action::Up) {
            movement.z -= MOVEMENT_SPEED;
        }
        if input.pressed(Action::Down) {
            movement.z += MOVEMENT_SPEED;
        }
        if input.pressed(Action::Left) {
            movement.x -= MOVEMENT_SPEED;
        }
        if input.pressed(Action::Right) {
            movement.x += MOVEMENT_SPEED;
        }

        movement = movement.clamp_length_max(MOVEMENT_SPEED);

        insert_or_modify(
            &mut commands,
            entity,
            &mut controller,
            || TnuaController::default(),
            |c| {
                c.basis(TnuaBuiltinWalk {
                    desired_velocity: movement,
                    desired_forward: movement.normalize_or_zero(),
                    float_height: PLAYER_HEIGHT / 2.,
                    ..default()
                });
            },
        );
    }
}

fn player_movement_ladder(
    mut commands: Commands,
    mut player: Query<
        (
            Entity,
            &ActionState<Action>,
            &PlayerMovingOnLadder,
            &mut Transform,
        ),
        With<Player>,
    >,
    time: Res<Time>,
) {
    const LADDER_SPEED: f32 = 2.0;

    for (entity, input, ladder, mut transform) in player.iter_mut() {
        // let frac = (transform.translation.y - ladder.ladder_bottom.y)
        //     / (ladder.ladder_top.y - ladder.ladder_bottom.y);

        let height = ladder.ladder_top.y - ladder.ladder_bottom.y;
        let cur_pos = transform.translation.y - ladder.ladder_bottom.y;

        if input.pressed(Action::Up) {
            if cur_pos > height + PLAYER_HEIGHT / 2. {
                commands
                    .entity(entity)
                    .remove::<PlayerMovingOnLadder>()
                    .insert(PlayerWalking);
                transform.translation -= ladder.ladder_normal * PLAYER_WIDTH * 0.8;
            } else {
                transform.translation += LADDER_SPEED * Vec3::Y * time.delta_seconds();
            }
        }
        if input.pressed(Action::Down) {
            if cur_pos < 0.1 {
                commands
                    .entity(entity)
                    .remove::<PlayerMovingOnLadder>()
                    .insert(PlayerWalking);
            } else {
                transform.translation -= LADDER_SPEED * Vec3::Y * time.delta_seconds();
            }
        }
    }
}

fn player_animation(
    mut player: Query<(&mut Handle<StandardMaterial>, &TnuaController), With<Player>>,
    player_images: Res<PlayerImages>,
    time: Res<Time>,
    mut walk_start_time: Local<Option<f32>>,
) {
    for (mut mat, controller) in player.iter_mut() {
        match controller.concrete_basis::<TnuaBuiltinWalk>() {
            None => {
                *mat = player_images.0[0].clone();
                continue;
            }
            Some(walk) => {
                let speed = walk.1.running_velocity.length();

                if speed == 0. {
                    *mat = player_images.0[0].clone();
                    continue;
                }

                let walk_start_time = if let Some(t) = *walk_start_time {
                    t
                } else {
                    let t = time.elapsed_seconds();
                    *walk_start_time = Some(t);
                    t
                };

                const WALK_ANIMATION_DURATION: f32 = 0.4;
                const WALK_ANIMATION_FRAMES: [(f32, usize); 2] = [(0.0, 0), (0.6, 1)];

                let m = ((time.elapsed_seconds() - walk_start_time) % WALK_ANIMATION_DURATION)
                    / WALK_ANIMATION_DURATION;

                for af in WALK_ANIMATION_FRAMES.into_iter().rev() {
                    if m >= af.0 {
                        *mat = player_images.0[af.1].clone();
                        break;
                    }
                }
            }
        }
    }
}

// Player camera

fn build_player_camera(app: &mut App) {
    app.add_plugins(LookTransformPlugin)
        .add_systems(Update, add_look_transform)
        .add_systems(Update, player_following_camera);
}

#[derive(Component, Debug)]
pub struct PlayerFollowingCamera;

fn add_look_transform(
    mut commands: Commands,
    player: Query<(Entity, &Transform), (Added<PlayerFollowingCamera>, Without<LookTransform>)>,
) {
    for (entity, transform) in player.iter() {
        commands.entity(entity).insert(LookTransformBundle {
            transform: LookTransform::new(transform.translation, Vec3::ZERO, Vec3::Y),
            smoother: Smoother::new(0.9),
        });
    }
}

fn player_following_camera(
    mut camera: Query<&mut LookTransform, With<PlayerFollowingCamera>>,
    player: Query<&GlobalTransform, With<Player>>,
) {
    let Ok(player) = player.get_single() else {
        return;
    };

    for mut camera in camera.iter_mut() {
        camera.target = player.translation();
    }
}

fn player_interaction(
    mut commands: Commands,
    ray: Query<(&RayCaster, &RayHits, &Parent), With<InteractionRayCaster>>,
    ladders: Query<(Entity, &Ladder, &Position, &Rotation, &Collider), Without<Player>>,
    mut player: Query<
        (
            Entity,
            &ActionState<Action>,
            Has<PlayerWalking>,
            &mut Transform,
        ),
        With<Player>,
    >,
) {
    for (ray, hits, parent) in &ray {
        screen_print!("hit: {:?}", hits.as_slice());

        let Ok((entity, action, walking, mut transform)) = player.get_mut(parent.get()) else {
            error!("Player missing");
            continue;
        };

        if action.just_pressed(Action::Interact) {
            if walking {
                for hit in hits.iter() {
                    if let Some((ladder_entity, ladder, ladder_pos, ladder_rot, col)) =
                        ladders.get(hit.entity).ok()
                    {
                        // align with the center of the ladder
                        let hit_pos =
                            ray.global_origin() + ray.global_direction() * hit.time_of_impact;
                        let ladder_center = (hit_pos - ladder_pos.0).dot(ladder.face_normal)
                            * ladder.face_normal
                            + ladder_pos.0;
                        let player_pos =
                            Vec3::new(ladder_center.x, transform.translation.y, ladder_center.z);
                        transform.translation = player_pos;
                        transform.rotation =
                            Quat::from_rotation_y(ladder.face_normal.xz().angle_between(Vec2::Y));

                        let aabb = col.compute_aabb(ladder_pos.0, ladder_rot.0);
                        let half_height = aabb.half_extents().y;
                        let center = aabb.center().y;
                        let (top, bottom) = (
                            center + half_height,
                            center - half_height + PLAYER_HEIGHT / 2.0,
                        );

                        commands.entity(entity).remove::<PlayerWalking>().insert(
                            PlayerMovingOnLadder {
                                ladder_normal: ladder.face_normal,
                                ladder_top: Vec3::new(player_pos.x, top, player_pos.z),
                                ladder_bottom: Vec3::new(player_pos.x, bottom, player_pos.z),
                            },
                        );

                        screen_print!("begin moving on ladder {ladder_entity:?}");
                        break;
                    }
                }
            } else {
                commands
                    .entity(entity)
                    .remove::<PlayerMovingOnLadder>()
                    .insert(PlayerWalking);
                screen_print!("end moving on ladder");
            }
        }
    }
}
