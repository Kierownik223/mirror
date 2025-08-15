# MARMAK Mirror

MARMAK Mirror is a website for hosting, browsing and managing files.

## Features

- Microsoft icons
- README Markdown rendering
- Folder size calculation
- Folder restriction (for logged in users)
- Folder masking (hide from directory listing, visible to admins)
- Multiple language support
- Plain HTML mode
- Wide browser compatibility (works as far back as NCSA Mosaic 1.0 if configured right)
- GDPR compliant
- Audio player with metadata
- Video player
- ZIP browser
- System information (disk usage, memory usage)
- File uploader
- Multiple themes

## Building

If the Rust compiler is not already installed, you can find out how [on their official website](https://www.rust-lang.org/tools/install).

```shell
git clone https://git.marmak.net.pl/MARMAK/mirror
cd mirror
cargo build --release
```

Once complete, the executable will be located at `./target/release/mirror` or `./target/release/mirror.exe` depending on your operating system.

## Configuration

### Mirror

The example config files show all available options. Before first startup, rename `config.toml.example` to `config.toml` and `Rocket.toml.example` to `Rocket.toml`. The files should be located in the working directory of the program

### Webservers

#### Caddy

Below is a configuration for Caddy (which is the recommended reverse-proxy for Mirror)

```
http://dl.example.com, https://dl.example.com {
    header /static/fonts/* Access-Control-Allow-Origin *

    route {
        header Cache-Control private
        header /api/* Cache-Control no-cache
        header /poster* Cache-Control public
    }

    handle /static/* {
        root * /path/to/mirror/files
        header /static* Cache-Control public
        header * -Onion-Location
        file_server
    }

    @xap path *.xap
    @appx path *.appx
    @appxbundle path *.appxbundle

    reverse_proxy :2115 {
            @dl header X-Send-File *
            handle_response @dl {
                    root * /path/to/mirror
                    rewrite * /{rp.header.X-Send-File}
                    method * GET

                    header * Cache-Control {rp.header.Cache-Control}

                    file_server

                    header @xap Content-Type "application/x-silverlight-app"
                    header @appx Content-Type "application/appx"
                    header @appxbundle Content-Type "application/appxbundle"
            }
    }

    redir /direct /account{uri} 302

    handle_errors 502 {
            respond "MARMAK Mirror is currently unavailable. Please try again later." 503
    }
}
```

#### Apache2, nginx, etc.

These webservers are unsupported as of now.

## Usage

Just start the program with `./target/release/mirror`, and it listens on http://localhost:2115. You can install it as a service, or use Docker (experimental).

## Acknowledgments

- [@Olek47](https://github.com/Olek47) for his ideas for the site.
- [@Microsoft](https://github.com/microsoft) for their work on the icons.

## Contributing

Pull requests are welcome. For major changes, please open an issue first to discuss what you would like to change.

## License

[GNU AGPL v3](https://www.gnu.org/licenses/agpl-3.0.en.html)
