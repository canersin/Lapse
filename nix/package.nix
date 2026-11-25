{
  lib,
  rustPlatform,
  pkg-config,
  makeBinaryWrapper,
  alsa-lib,
  dbus,
  libGL,
  libxkbcommon,
  wayland,
  libx11,
  libxcursor,
  libxrandr,
  libxi,
  ffmpeg,
}:

let
  cargoToml = lib.importTOML ../Cargo.toml;
  # Loaded at start-up (graphics, windows, keyboard), not linked.
  runtimeLibs = [
    libGL
    libxkbcommon
    wayland
    dbus
    libx11
    libxcursor
    libxrandr
    libxi
  ];
in
rustPlatform.buildRustPackage {
  pname = "lapse";
  inherit (cargoToml.package) version;

  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../src
      ../assets
    ];
  };

  cargoLock.lockFile = ../Cargo.lock;

  nativeBuildInputs = [
    pkg-config
    makeBinaryWrapper
  ];
  buildInputs = [
    alsa-lib
    dbus
  ];

  # The real binary keeps the name "lapse": Lapse finds its own service by it.
  # gpu-screen-recorder comes from the system (on NixOS with the capability it
  # needs, see programs.gpu-screen-recorder); ffmpeg is added after it.
  postInstall = ''
    mkdir -p $out/libexec/lapse
    mv $out/bin/Lapse $out/libexec/lapse/lapse
    makeWrapper $out/libexec/lapse/lapse $out/bin/lapse \
      --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath runtimeLibs} \
      --suffix PATH : ${lib.makeBinPath [ ffmpeg ]}

    install -Dm644 assets/lapse.desktop $out/share/applications/lapse.desktop
    install -Dm644 assets/icon.png $out/share/icons/hicolor/512x512/apps/lapse.png
    install -Dm644 assets/lapse.svg $out/share/icons/hicolor/scalable/apps/lapse.svg
    install -Dm644 assets/lapse-symbolic.svg $out/share/icons/hicolor/symbolic/apps/lapse-symbolic.svg
  '';

  meta = {
    description = "Save the last moments of your game and trim clips";
    homepage = "https://github.com/canersin/Lapse";
    license = lib.licenses.gpl3Only;
    platforms = [ "x86_64-linux" ];
    mainProgram = "lapse";
  };
}
