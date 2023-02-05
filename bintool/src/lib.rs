#[test]
fn test() {
    let a = "/dev/tty";
    let s = a.split('/');
    for i in s {
        println!("{}", i);
    }
}
