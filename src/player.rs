use bevy::ecs::system::EntityCommands;
use bevy::prelude::*;
use bevy_debug_text_overlay::screen_print;
use bevy_tnua::{builtins::TnuaBuiltinWalk, controller::TnuaController, TnuaUserControlsSystemSet};
use bevy_tnua::{control_helpers::TnuaCrouchEnforcerPlugin, prelude::*};
use bevy_tnua_xpbd3d::*;
use bevy_xpbd_3d::prelude::*;
use leafwing_input_manager::prelude::*;
use seldom_state::prelude::*;
use seldom_state::trigger::AndTrigger;
use smooth_bevy_cameras::{LookTransform, LookTransformBundle, LookTransformPlugin, Smoother};

use crate::terrain::Ladder;

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, startup);
        build_player_add(app);
        build_movement(app);
        build_player_camera(app);

        app.add_plugins(StateMachinePlugin::default());
        app.add_event::<LadderInteractionBeginEvent>()
            .add_event::<LadderInteractionEndEvent>();
        // Required to apply LinearVelocity
        app.add_systems(
            Update,
            apply_deferred
                .after(seldom_state::set::StateSet::Transition)
                .before(TnuaPipelineStages::Motors),
        );
        app.add_systems(Update, player_interaction);
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

#[derive(Component, Clone, Debug)]
struct PlayerGrounded;

#[derive(Component, Clone, Debug)]
struct PlayerJumping;

#[derive(Component, Clone, Debug)]
struct PlayerMovingOnLadder {
    face_normal: Vec3,
    top: Vec3,
    bottom: Vec3,
}

#[derive(Event, Clone)]
struct LadderInteractionBeginEvent {
    entity: Entity,
    face_normal: Vec3,
    top: Vec3,
    bottom: Vec3,
}

#[derive(Event, Clone)]
struct LadderInteractionEndEvent(Entity);

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
            .insert(player_state_machine(entity))
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
        add_action_state(commands.entity(entity));
    }
}

struct IsJumping;

impl Trigger for IsJumping {
    type Param<'w, 's> = Query<'w, 's, &'static TnuaController>;

    type Ok = ();

    type Err = ();

    fn trigger(
        &self,
        entity: Entity,
        query: <<Self as Trigger>::Param<'_, '_> as bevy::ecs::system::SystemParam>::Item<'_, '_>,
    ) -> Result<Self::Ok, Self::Err> {
        query
            .get(entity)
            .unwrap()
            .concrete_action::<TnuaBuiltinJump>()
            .map(|_| ())
            .ok_or(())
    }
}

fn player_state_machine(entity: Entity) -> impl Bundle {
    let initial = PlayerGrounded;
    let state_machine = StateMachine::default()
        .trans::<PlayerGrounded>(JustPressedTrigger(Action::Jump), PlayerJumping)
        .trans::<PlayerJumping>(
            AndTrigger(IsJumping, PressedTrigger(Action::Jump)),
            PlayerJumping,
        )
        .trans::<PlayerJumping>(
            AndTrigger(IsJumping.not(), PressedTrigger(Action::Jump).not()),
            PlayerGrounded,
        )
        .trans_builder::<PlayerGrounded, _, PlayerMovingOnLadder>(
            EventTrigger::<LadderInteractionBeginEvent>::default(),
            move |_prev, ev| {
                if ev.entity != entity {
                    return None;
                }
                Some(PlayerMovingOnLadder {
                    face_normal: ev.face_normal,
                    top: ev.top,
                    bottom: ev.bottom,
                })
            },
        )
        .trans_builder::<PlayerMovingOnLadder, _, PlayerGrounded>(
            EventTrigger::<LadderInteractionEndEvent>::default(),
            move |_prev, ev| {
                if ev.0 != entity {
                    return None;
                }
                Some(PlayerGrounded)
            },
        )
        .on_enter::<PlayerMovingOnLadder>(|entity| {
            entity
                .remove::<TnuaControllerBundle>()
                .insert(RigidBody::Kinematic)
                .insert((LinearVelocity::ZERO, AngularVelocity::ZERO));
        })
        .on_exit::<PlayerMovingOnLadder>(|entity| {
            entity
                .insert(TnuaControllerBundle::default())
                .insert(RigidBody::Dynamic);
        });

    (initial, state_machine)
}

// Player movement

fn build_movement(app: &mut App) {
    app.add_plugins((
        TnuaXpbd3dPlugin,
        TnuaControllerPlugin,
        TnuaCrouchEnforcerPlugin,
    ))
    .add_plugins(InputManagerPlugin::<Action>::default())
    .add_systems(
        FixedUpdate,
        (player_jumping, player_movement_walk, player_movement_ladder)
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

fn add_action_state(mut entity: EntityCommands) {
    entity.insert(InputManagerBundle::<Action> {
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

fn player_jumping(
    mut player: Query<
        (
            Ref<PlayerJumping>,
            &ActionState<Action>,
            &mut TnuaController,
        ),
        (With<Player>, With<PlayerJumping>),
    >,
) {
    for (jumping_state, input, mut controller) in player.iter_mut() {
        if jumping_state.is_added() || input.pressed(Action::Jump) {
            controller.action(TnuaBuiltinJump {
                height: 1.0,
                ..default()
            });
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
        (With<Player>, Or<(With<PlayerGrounded>, With<PlayerJumping>)>),
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
    mut ladder_end: EventWriter<LadderInteractionEndEvent>,
) {
    const LADDER_SPEED: f32 = 2.0;

    for (entity, input, ladder, mut transform) in player.iter_mut() {
        // let frac = (transform.translation.y - ladder.ladder_bottom.y)
        //     / (ladder.ladder_top.y - ladder.ladder_bottom.y);

        let height = ladder.top.y - ladder.bottom.y;
        let cur_pos = transform.translation.y - ladder.bottom.y;

        if input.pressed(Action::Up) {
            if cur_pos > height + PLAYER_HEIGHT / 2. {
                ladder_end.send(LadderInteractionEndEvent(entity));
                transform.translation -= ladder.face_normal * PLAYER_WIDTH * 0.8;
            } else {
                transform.translation += LADDER_SPEED * Vec3::Y * time.delta_seconds();
            }
        }
        if input.pressed(Action::Down) {
            if cur_pos < 0.1 {
                ladder_end.send(LadderInteractionEndEvent(entity));
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
    ray: Query<(&RayCaster, &RayHits, &Parent), With<InteractionRayCaster>>,
    ladders: Query<(Entity, &Ladder, &Position, &Rotation, &Collider), Without<Player>>,
    mut player: Query<(&ActionState<Action>, Has<PlayerGrounded>, &mut Transform), With<Player>>,
    mut ladder_begin: EventWriter<LadderInteractionBeginEvent>,
    mut ladder_end: EventWriter<LadderInteractionEndEvent>,
) {
    for (ray, hits, parent) in &ray {
        screen_print!("hit: {:?}", hits.as_slice());

        let player_entity = parent.get();

        let Ok((action, walking, mut transform)) = player.get_mut(player_entity) else {
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

                        ladder_begin.send(LadderInteractionBeginEvent {
                            entity: player_entity,
                            face_normal: ladder.face_normal,
                            top: Vec3::new(player_pos.x, top, player_pos.z),
                            bottom: Vec3::new(player_pos.x, bottom, player_pos.z),
                        });

                        screen_print!("begin moving on ladder {ladder_entity:?}");
                        break;
                    }
                }
            } else {
                ladder_end.send(LadderInteractionEndEvent(player_entity));
                screen_print!("end moving on ladder");
            }
        }
    }
}
