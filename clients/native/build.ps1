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
$savedEnv = @{}
$managedVars = @(
    'CARGO_HOME','CARGO_TARGET_DIR','PUB_CACHE','GRADLE_USER_HOME','ANDROID_USER_HOME',
    'XDG_CACHE_HOME','XDG_CONFIG_HOME','FLUTTER_SUPPRESS_ANALYTICS','DART_SUPPRESS_ANALYTICS',
    'TEMP','TMP','SINGBOARD_RELEASE_VERSION','SINGBOARD_SERVICE_HOST',
    'CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER','CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS',
    'CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER','CC_aarch64_linux_android','AR_aarch64_linux_android',
    'CARGO_TARGET_ARMV7_LINUX_ANDROIDEABI_LINKER','CC_armv7_linux_androideabi','AR_armv7_linux_androideabi'
)
foreach ($var in $managedVars) {
    $savedEnv[$var] = [Environment]::GetEnvironmentVariable($var, 'Process')
}
try {
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
    $sysroot = (rustc --print sysroot).Trim()
    $lld = Join-Path $sysroot "lib\rustlib\$target\bin\rust-lld.exe"
    if (Test-Path -LiteralPath $lld) {
        $env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER = $lld
        $env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS = '-Ctarget-feature=+crt-static -Clinker-flavor=lld-link'
    } else {
        Remove-Item Env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER -ErrorAction SilentlyContinue
        $env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS = '-Ctarget-feature=+crt-static'
    }
    Invoke-Tool cargo @('build','--locked','--profile','windows-release','--target',$target,'--manifest-path',$manifest,'-p','singboard-service')
    $hostExe = "$cache/singboard-service.exe"
    Copy-Item -LiteralPath "$env:CARGO_TARGET_DIR/$target/windows-release/singboard-service.exe" -Destination $hostExe -Force
    $env:SINGBOARD_SERVICE_HOST = $hostExe
    Invoke-Tool cargo @('build','--locked','--profile','windows-release','--target',$target,'--manifest-path',$manifest,'-p','singboard-gpui')
    $binary = "$env:CARGO_TARGET_DIR/$target/windows-release/singboard.exe"
    if ($ReleaseAssets) {
        Copy-Item -LiteralPath $binary -Destination "$outDir/singboard-windows-x64.exe" -Force
        return
    }
    foreach ($name in @('singboard.exe','singboard-preview.exe','singboard-windows-x64.exe')) {
        try {
            Copy-Item -LiteralPath $binary -Destination "$outDir/$name" -Force
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
$ndk = $env:ANDROID_NDK_HOME
if (!$ndk) { $ndk = "$env:ANDROID_HOME/ndk/28.2.13676358" }
$ndkVersion = Split-Path -Leaf $ndk
$stage = "$cache/android-app"
if (!(Test-Path "$stage/android/gradlew.bat")) {
    Invoke-Tool flutter @('create','--no-pub','--empty','--platforms=android','--org','io.github.yuu518','--project-name','singboard_native',$stage)
}
Copy-Item -LiteralPath "$PSScriptRoot/android/pubspec.yaml" -Destination $stage -Force
if (Test-Path "$PSScriptRoot/android/pubspec.lock") {
    Copy-Item -LiteralPath "$PSScriptRoot/android/pubspec.lock" -Destination $stage -Force
}
if (Test-Path "$stage/lib") {
    Remove-Item -Path "$stage/lib/*.dart" -Force -ErrorAction SilentlyContinue
}
Copy-Item -Path "$PSScriptRoot/android/lib/*.dart" -Destination "$stage/lib" -Force
if (Test-Path "$stage/assets") {
    Remove-Item -LiteralPath "$stage/assets" -Recurse -Force
}
Copy-Item -LiteralPath "$PSScriptRoot/android/assets" -Destination $stage -Recurse -Force
Copy-Item -LiteralPath "$PSScriptRoot/android/AndroidManifest.xml" -Destination "$stage/android/app/src/main" -Force
$null = New-Item -ItemType Directory -Force "$stage/android/app/src/main/res/xml"
Copy-Item -LiteralPath "$PSScriptRoot/android/update_paths.xml" -Destination "$stage/android/app/src/main/res/xml" -Force
if (Test-Path "$stage/android/app/src/main/kotlin") {
    Remove-Item -LiteralPath "$stage/android/app/src/main/kotlin" -Recurse -Force
}
$kotlinDir = "$stage/android/app/src/main/kotlin/io/github/Yuu518/singboard"
$null = New-Item -ItemType Directory -Force $kotlinDir
Copy-Item -LiteralPath "$PSScriptRoot/android/MainActivity.kt" -Destination "$kotlinDir/MainActivity.kt" -Force
if (Test-Path "$stage/android/app/src/main/res/mipmap-anydpi-v26") {
    Remove-Item -LiteralPath "$stage/android/app/src/main/res/mipmap-anydpi-v26" -Recurse -Force
}
Copy-Item -Path "$PSScriptRoot/android/res/*" -Destination "$stage/android/app/src/main/res" -Recurse -Force
$gradleApp = "$stage/android/app/build.gradle.kts"
$gradleText = (Get-Content -LiteralPath $gradleApp -Raw)
$gradleText = $gradleText -replace 'namespace = "[^"]+"','namespace = "io.github.Yuu518.singboard"'
$gradleText = $gradleText -replace 'applicationId = "[^"]+"','applicationId = "io.github.Yuu518.singboard"'
$gradleText = $gradleText.Replace('minSdk = flutter.minSdkVersion','minSdk = 26')
$gradleText = $gradleText -replace 'ndkVersion = (flutter\.ndkVersion|"[^"]+")',"ndkVersion = `"$ndkVersion`""
[IO.File]::WriteAllText($gradleApp,$gradleText)
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
    Copy-Item -LiteralPath "$env:CARGO_TARGET_DIR/$($arch.target)/release/libsingboard_stream.so" -Destination $jni -Force
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
        Copy-Item -LiteralPath $apk -Destination "$outDir/singboard-$abi.apk" -Force
    }
} finally {
    Pop-Location
}
} finally {
    foreach ($var in $managedVars) {
        if ($null -eq $savedEnv[$var]) {
            Remove-Item "Env:$var" -ErrorAction SilentlyContinue
        } else {
            [Environment]::SetEnvironmentVariable($var, $savedEnv[$var], 'Process')
        }
    }
}
