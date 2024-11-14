use core::add;

struct Service1 {
    name: String,
}

// impl Service1 {
//     fn new(name: &str) -> Self {
//         Service1 {
//             name: name.to_string(),
//         }
//     }
// }
//
// impl core::service::service::ServiceType for Service1 {
//     fn say_hi(&self) -> String {
//         println!("my name is: {}", &self.name);
//         self.name.clone()
//     }
// }

// many variants of type
struct TypeA {
    name: String,
}

struct TypeB {
    name: String,
}

// example of trait that needs to be implemented
trait NeedImpl {
    fn test_call(&self) -> String;
}

// the concrete type
struct Concrete {
    name: String,
}

// to make concrete work with type a
impl From<TypeA> for Concrete {
    fn from(t: TypeA) -> Self {
        Concrete {
            name: t.name,
        }
    }
}

// to make concrete work with type b
impl From<TypeB> for Concrete {
    fn from(t: TypeB) -> Self {
        Concrete {
            name: t.name,
        }
    }
}

// make the concrete implement the needs
impl NeedImpl for Concrete {
    fn test_call(&self) -> String {
        self.name.clone()
    }
}

// super trait as type that needs to implement the needs
trait SuperTrait: NeedImpl {}

// implement the super trait where t need to implement the needs
impl<T: NeedImpl> SuperTrait for T {}

// wrapped the super trait as dynamic type
type Types = Box<dyn SuperTrait>;

fn get_type_a() -> Types {
    let type_a = TypeA {
        name: String::from("from type a"),
    };
    let concrete = Concrete::from(type_a);
    let dynamic_type: Types = Box::new(concrete);
    dynamic_type
}

fn get_type_b() -> Types {
    let type_b = TypeB {
        name: String::from("from type b"),
    };
    let concrete = Concrete::from(type_b);
    let dynamic_type: Types = Box::new(concrete);
    dynamic_type
}

fn say_name(input_type: Types) {
    let name = input_type.test_call();
    print!("{}", name);
}

fn main() {
    let test_a = get_type_a();
    say_name(test_a);
    let test_b = get_type_b();
    say_name(test_b);
    // println!("Hello, world!");
    // println!("2 + 2 is: {}", add(2, 2));
    // let mut service1 = core::service::service::Service::new("service-1", Service1::new("athaya"));
    // service1.add_tcp_network("127.0.0.1:8500");
    // service1.add_tcp_network("127.0.0.1:8600");
    // let mut server = core::server::server::Server::new();
    // server.add_service(service1);
    // server.run_forever();
}
