This is vex.

Commands: {

  serve -> serve a local server at localhost:45311
  fetch <pkg> -> fetch pkg
  build <pkg>.tar -> build pkg locally 
  rebuild -> rebuild from pkgs.vex from repos on config.vex
  list -> list available packages for install from all repos 
  version -> version 
  portserve <port> -> serve on specific port
  refresh -> refresh cache 
  add/remove <pkg> -> add/remove pkg from pkgs.vex, sync manually
  search <pkg> -> contains search for pkg 
  exists <pkg> -> == search for pkg 
}

vex_lang syntax: (.vex) {
  
  key {
    "entry"
    "entry0"
    "entry1"
  }
  key0 {
    "entry"
    "entry0"
    "entry1"
  }
  ...

}

example config.vex :

```
repos {
  "http://localhost:45311"
  "http://foo.some_server.bar"
}
```

example pkgs.vex :
```
packages {
  "neovim"
  "xenon"
  "foo"
  "bar"
}
```

all .tar within current directory will be served on portserve and serve.

the repo must have pkgs.list in their immediate root. 

example :
 some-repo/
```
```
```my-repo/
  pkgs.list 
  some.tar 
  other.tar 
  
```

example pkgs.list: 
``` 
some.tar v1.0.0 
other.tar v0.1.2 
```



the .tar must have build.vex within their immediate root.

example :

foo.tar / 
  build.vex 
  src/
    main.rs 
  Cargo.toml 
  Cargo.lock
  install.sh*
  .gitignore

example build.vex :
```

name {
  "sample"
}

version {
  "0.1.0"
}

dependencies {
  "xenon"
  "neovim"
}

commands {
  "echo installing"
  "cargo build"
  "./install.sh"
}
// where do you install to? (for parellel builds)
install-to {
  "/vex/bin"
  "/vex/lib"

}
```

do not ask queries. I do not have enough free time. This is a hobby project for all.
