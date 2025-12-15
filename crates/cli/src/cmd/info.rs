use syslua_lib::platform::platform_triple;

pub fn cmd_info() {
  println!("System:");
  match platform_triple() {
    Some(triple) => println!("Platform: {}", triple),
    _ => println!("Could not detect platform."),
  }
}
