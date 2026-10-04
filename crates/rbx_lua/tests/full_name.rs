//! `Instance:GetFullName`, against the semantics in Roblox's reference: names
//! joined by periods, the `DataModel` left out, an unparented tree starting at
//! its topmost ancestor, no escaping of periods in names.

use rbx_lua::Runtime;
use rbx_reflection::ReflectionDatabase;

const PLACE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/tests/TestPlace.rbxl"
));

fn full_names(script: &str) -> String {
    let dom = rbx_binary::deserialize(PLACE).expect("fixture must deserialize");
    let mut runtime =
        Runtime::new(dom, ReflectionDatabase::embedded()).expect("runtime must start");
    runtime.run(script).expect("script must run").text()
}

#[test]
fn a_nested_workspace_part_is_named_from_its_service() {
    let text = full_names(
        r#"
        local folder = Instance.new("Folder", workspace)
        folder.Name = "A"
        local part = Instance.new("Part", folder)
        part.Name = "B"
        print(part:GetFullName())
        "#,
    );
    assert_eq!(text, "Workspace.A.B");
}

#[test]
fn services_and_terrain_leave_the_datamodel_out() {
    let text = full_names(
        r#"
        print(workspace:GetFullName())
        print(workspace.Terrain:GetFullName())
        print(game:GetService("Workspace"):GetFullName())
        "#,
    );
    assert_eq!(text, "Workspace\nWorkspace.Terrain\nWorkspace");
}

#[test]
fn unparented_instances_start_at_their_topmost_ancestor() {
    let text = full_names(
        r#"
        local model = Instance.new("Model")
        local part = Instance.new("Part", model)
        print(model:GetFullName())
        print(part:GetFullName())
        "#,
    );
    assert_eq!(text, "Model\nModel.Part");
}

#[test]
fn game_names_itself_and_periods_are_not_escaped() {
    let text = full_names(
        r#"
        local part = Instance.new("Part", workspace)
        part.Name = "Door.Left"
        print(game:GetFullName())
        print(part:GetFullName())
        "#,
    );
    assert_eq!(text, "game\nWorkspace.Door.Left");
}
