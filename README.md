Vex is a decentralized userspace package manager.

Anyone can host a repo for it easily.

It does not need sudo/root access, it does thing in the userspace, unless the package maintainer uses sudo within their package.

There is no central point of failure, if a repo goes down, it is not like the package manager is now useless, another repo can easily be created and vex-pkg will still be usable.
```
Commands: {

  serve -> serve a local server at localhost:45311

  fetch <pkg> -> fetch pkg

  build <pkg>.tar -> build pkg locally 
  
  sync -> rebuild from pkgs.vex from repos on config.vex
  
  list -> list available packages for install from all repos 
  
  version -> version 
  
  portserve <port> -> serve on specific port
  
  refresh -> refresh cache 
  
  add/remove <pkg> -> add/remove pkg from pkgs.vex, sync manually, or add the -s|--sync flag / sync-on-add-remove in config.vex to sync automatically.
  
  search <pkg> -> contains search for pkg 
  
  exists <pkg> -> == search for pkg 

  info <pkg> -> get info on a pkg

}
```
vex_lang syntax: (.vex) {
  ```
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
  ```

}

example config.vex :

```
repos {
  "http://localhost:45311"
  "http://foo.some_server.bar"
}
refresh-time {
    "120"
}
sync-on-add-remove {
    "true"
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

the repo must have a /checksums folder with the .sha256 checksums of all pkgs. (used to verify package integrity)

example :
 
```
my-repo/
  pkgs.list 
  some.tar 
  other.tar 
  checksums/
    some.sha256
    other.sha256
  
```

example pkgs.list: 
``` 
some.tar v1.0.0 deps=
other.tar v0.1.2 deps=some
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
I recommend you don't package cargo projects, since they have parallelism within themselves, and vex-pkg also does parallel installs/compiles.

