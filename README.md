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
- Private folders
- File sharing when logged in
- Customisable upload limits

## Building

### Standalone

If the Rust compiler is not already installed, you can find out how [on their official website](https://www.rust-lang.org/tools/install).

```shell
git clone https://git.marmak.net.pl/MARMAK/mirror
cd mirror
cargo build --release
```

Once complete, the executable will be located at `./target/release/mirror` or `./target/release/mirror.exe` depending on your operating system.

### Docker

You will need Docker and Docker compose for the optimal experience.

```shell
git clone https://git.marmak.net.pl/MARMAK/mirror
cd mirror
docker build -t mirror .
```

Once building is done, either run the image as is, or use Docker compose.

```shell
docker compose up -d
```

The container exposes port 8080, the default Docker compose file has that port mapped to 2115.

## Configuration

### Mirror

The example config files show all available options.  
Before first startup, copy `config.toml.example` to `config.toml` and `Rocket.toml.example` to `Rocket.toml`. The files should be located in the working directory of the program. If you choose not to do so, a minimal working environment will be used.
The Mirror can also be configured with environment variables. The available options are in the examle config files, you just add `MIRROR_` to the beginning and write it out all uppercase.

### Webservers

#### Caddy

An example configuration is provided as a Caddyfile.

#### Apache2

Change `standalone` to `true` in the `config.toml` file or set the `MIRROR_STANDALONE` environment variable to `true` and use this config:

```
<VirtualHost *:80>
	ProxyPreserveHost On
	ProxyPass / http://127.0.0.1:2115/
	ProxyPassReverse / http://127.0.0.1:2115/

	ProxyErrorOverride Off
</VirtualHost>
```

#### nginx

Change `standalone` to `true` in the `config.toml` file or set the `MIRROR_STANDALONE` environment variable to `true` and use the [standard reverse proxy config](https://docs.nginx.com/nginx/admin-guide/web-server/reverse-proxy/) with the address of `127.0.0.1:2115`

## Usage

Just start the program with `./target/release/mirror`, and it listens on http://localhost:2115. You can install it as a service, or use Docker, as shown above.

## Acknowledgments

- [@Olek47](https://github.com/Olek47) for his ideas for the site.
- [@Microsoft](https://github.com/microsoft) for their work on the icons.

## Contributing

Pull requests are welcome. For major changes, please open an issue first to discuss what you would like to change.

## License

[GNU AGPL v3](https://www.gnu.org/licenses/agpl-3.0.en.html)
