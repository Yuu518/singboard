param(
    [Parameter(Mandatory)][ValidateSet('Windows','Android')][string]$Platform,
    [string]$ReleaseVersion,
    [switch]$ReleaseAssets
)

$ErrorActionPreference = 'Stop'
$project = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$cache = Join-Path $project '.build-deps'
$null = git -C $project check-ignore --no-index .build-deps/probe
if ($LASTEXITCODE) {
    $exclude = git -C $project rev-parse --path-format=absolute --git-path info/exclude
    Add-Content -LiteralPath $exclude -Value "`n.build-deps/"
}
$env:CARGO_HOME = "$cache/cargo"
$env:CARGO_TARGET_DIR = "$cache/target"
$env:PUB_CACHE = "$cache/pub"
$env:GRADLE_USER_HOME = "$cache/gradle"
$env:ANDROID_USER_HOME = "$cache/android-user"
$env:XDG_CACHE_HOME = "$cache/xdg-cache"
$env:XDG_CONFIG_HOME = "$cache/xdg-config"
$env:FLUTTER_SUPPRESS_ANALYTICS = 'true'
$env:DART_SUPPRESS_ANALYTICS = 'true'
$env:TEMP = "$cache/tmp"
$env:TMP = $env:TEMP
$outDir = "$project/dist/native"
$null = New-Item -ItemType Directory -Force $env:TEMP,$outDir

function Invoke-Tool([string]$Name, [string[]]$Arguments) {
    & $Name @Arguments
    if ($LASTEXITCODE) { throw "$Name 执行失败 ($LASTEXITCODE)" }
}

$manifest = "$PSScriptRoot/Cargo.toml"
if (!$ReleaseVersion) {
    $ReleaseVersion = (Select-String -LiteralPath "$PSScriptRoot/stream/Cargo.toml" -Pattern '^version = "([^"]+)"$').Matches.Groups[1].Value
}
if ($ReleaseVersion -notmatch '^(0|[1-9][0-9]{0,2})\.(0|[1-9][0-9]?)\.(0|[1-9][0-9]?)$') {
    throw "原生发行版本需要使用 major.minor.patch，次版本与修订号均须小于 100"
}
$buildNumber = 10000 * [int]$Matches[1] + 100 * [int]$Matches[2] + [int]$Matches[3]
$env:SINGBOARD_RELEASE_VERSION = $ReleaseVersion
if ($Platform -eq 'Windows') {
    $target = 'x86_64-pc-windows-msvc'
    $env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS = '-Ctarget-feature=+crt-static'
    Invoke-Tool cargo @('build','--locked','--release','--target',$target,'--manifest-path',$manifest,'-p','singboard-service','--target-dir',"$cache/host")
    $env:SINGBOARD_SERVICE_HOST = "$cache/host/$target/release/singboard-service.exe"
    Invoke-Tool cargo @('build','--locked','--profile','windows-release','--target',$target,'--manifest-path',$manifest,'-p','singboard-gpui')
    $binary = "$env:CARGO_TARGET_DIR/$target/windows-release/singboard.exe"
    if ($ReleaseAssets) {
        Copy-Item -LiteralPath $binary -Destination "$outDir/singboard-native-windows-x64.exe"
        return
    }
    foreach ($name in @('singboard.exe','singboard-preview.exe','singboard-native-windows-x64.exe')) {
        try {
            Copy-Item -LiteralPath $binary -Destination "$outDir/$name"
            Write-Output "已生成 $outDir/$name"
            return
        } catch [System.IO.IOException] {
            if (($_.Exception.HResult -band 0xffff) -notin @(32,33)) { throw }
        }
    }
    throw "三个输出文件均在使用。已构建的单 EXE 位于 $binary"
}

if ($ReleaseAssets -and !$env:SINGBOARD_ANDROID_KEYSTORE) { throw '正式 APK 需要设置 SINGBOARD_ANDROID_KEYSTORE' }
if ($env:SINGBOARD_ANDROID_KEYSTORE) {
    $keystore = (Resolve-Path -LiteralPath $env:SINGBOARD_ANDROID_KEYSTORE).Path
    foreach ($key in @('ANDROID_KEYSTORE_PASSWORD','ANDROID_KEY_ALIAS','ANDROID_KEY_PASSWORD')) {
        if (![Environment]::GetEnvironmentVariable($key)) { throw "签名缺少环境变量 $key" }
    }
    $signer = (Resolve-Path -LiteralPath "$env:ANDROID_HOME/build-tools/36.0.0/apksigner.bat").Path
}
$stage = "$cache/android-app"
if (!(Test-Path "$stage/android/gradlew.bat")) {
    Invoke-Tool flutter @('create','--no-pub','--empty','--platforms=android','--org','io.github.yuu518','--project-name','singboard_native',$stage)
}
Copy-Item -LiteralPath "$PSScriptRoot/android/pubspec.yaml" -Destination $stage
if (Test-Path "$PSScriptRoot/android/pubspec.lock") {
    Copy-Item -LiteralPath "$PSScriptRoot/android/pubspec.lock" -Destination $stage
}
Copy-Item -Path "$PSScriptRoot/android/lib/*.dart" -Destination "$stage/lib"
Copy-Item -LiteralPath "$PSScriptRoot/android/assets" -Destination $stage -Recurse -Force
Copy-Item -LiteralPath "$PSScriptRoot/android/AndroidManifest.xml" -Destination "$stage/android/app/src/main"
$null = New-Item -ItemType Directory -Force "$stage/android/app/src/main/res/xml"
Copy-Item -LiteralPath "$PSScriptRoot/android/update_paths.xml" -Destination "$stage/android/app/src/main/res/xml"
Copy-Item -LiteralPath "$PSScriptRoot/android/MainActivity.kt" -Destination "$stage/android/app/src/main/kotlin/io/github/yuu518/singboard_native/MainActivity.kt"
$resources = "$PSScriptRoot/android/res"
foreach ($density in @('mdpi','hdpi','xhdpi','xxhdpi','xxxhdpi')) {
    $icon = "mipmap-$density/ic_launcher.png"
    Copy-Item -LiteralPath "$resources/$icon" -Destination "$stage/android/app/src/main/res/$icon"
}
$gradleApp = "$stage/android/app/build.gradle.kts"
$gradleText = (Get-Content -LiteralPath $gradleApp -Raw).Replace('minSdk = flutter.minSdkVersion','minSdk = 26')
[IO.File]::WriteAllText($gradleApp,$gradleText)
$ndk = $env:ANDROID_NDK_HOME
if (!$ndk) { $ndk = "$env:ANDROID_HOME/ndk/28.2.13676358" }
$bin = "$ndk/toolchains/llvm/prebuilt/windows-x86_64/bin"

foreach ($arch in @(
    @{target='aarch64-linux-android'; clang='aarch64-linux-android'; abi='arm64-v8a'},
    @{target='armv7-linux-androideabi'; clang='armv7a-linux-androideabi'; abi='armeabi-v7a'}
)) {
    $compiler = (Resolve-Path "$bin/$($arch.clang)26-clang.cmd").Path
    $key = $arch.target.Replace('-','_')
    Set-Item "Env:CARGO_TARGET_$($key.ToUpperInvariant())_LINKER" $compiler
    Set-Item "Env:CC_$key" $compiler
    Set-Item "Env:AR_$key" "$bin/llvm-ar.exe"
    Invoke-Tool cargo @('build','--locked','--release','--manifest-path',$manifest,'-p','singboard-stream','--target',$arch.target)
    $jni = "$stage/android/app/src/main/jniLibs/$($arch.abi)"
    $null = New-Item -ItemType Directory -Force $jni
    Copy-Item -LiteralPath "$env:CARGO_TARGET_DIR/$($arch.target)/release/libsingboard_stream.so" -Destination $jni
}

Push-Location $stage
try {
    Invoke-Tool flutter @('pub','get')
    Invoke-Tool flutter @('analyze','--no-pub','lib')
    Invoke-Tool flutter @('build','apk','--release','--no-pub','--split-per-abi','--target-platform','android-arm,android-arm64','--build-name',$ReleaseVersion,'--build-number',"$buildNumber")
    foreach ($abi in @('arm64-v8a','armeabi-v7a')) {
        $apk = "build/app/outputs/flutter-apk/app-$abi-release.apk"
        if ($keystore) {
            $signed = "$cache/signed-$abi.apk"
            Invoke-Tool $signer @('sign','--ks',$keystore,'--ks-key-alias',$env:ANDROID_KEY_ALIAS,'--ks-pass','env:ANDROID_KEYSTORE_PASSWORD','--key-pass','env:ANDROID_KEY_PASSWORD','--v4-signing-enabled','false','--out',$signed,$apk)
            Invoke-Tool $signer @('verify',$signed)
            $apk = $signed
        }
        Copy-Item -LiteralPath $apk -Destination "$outDir/singboard-native-$abi.apk"
    }
} finally {
    Pop-Location
}
