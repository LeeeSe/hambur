import java.io.File
import java.util.Properties
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.plugin.compose")
}

val uniffiVersion = "0.31.2"
val androidNdkVersion = "30.0.15729638"
val rustDir = rootProject.layout.projectDirectory.dir("rust")
val uniffiUdl = rustDir.file("crates/hambur-uniffi/src/hambur_uniffi.udl")
val uniffiConfig = rustDir.file("crates/hambur-uniffi/uniffi.toml")
val generatedUniffiDir = layout.buildDirectory.dir("generated/source/uniffi/kotlin")
// debug / release 必须各自独占一个输出目录。
// 两个 Rust 任务的输入完全相同,共用一个目录时 Gradle 无法区分它们:
// 跑完 assembleRelease 后 buildRustDebug 会被判为 UP-TO-DATE,
// 于是 debug APK 里嵌进 release 版 .so(实测体积从 48.9MB 掉到 42.2MB),反之亦然。
val generatedJniLibsDebugDir = layout.buildDirectory.dir("generated/rustJniLibs/debug")
val generatedJniLibsReleaseDir = layout.buildDirectory.dir("generated/rustJniLibs/release")
val uniffiBindgenRoot = layout.buildDirectory.dir("uniffi-bindgen")
val uniffiBindgenBin = uniffiBindgenRoot.map { it.file("bin/uniffi-bindgen").asFile }
val localPropertiesFile = rootProject.layout.projectDirectory.file("local.properties").asFile
val localProperties = Properties().apply {
    if (localPropertiesFile.isFile) {
        localPropertiesFile.inputStream().use { load(it) }
    }
}
val androidSdkDir = providers.environmentVariable("ANDROID_HOME")
    .orElse(providers.environmentVariable("ANDROID_SDK_ROOT"))
    .orElse(
        providers.provider<String> {
            localProperties.getProperty("sdk.dir")
                ?: error("Android SDK path missing. Set ANDROID_HOME or sdk.dir in local.properties.")
        },
    )
val androidNdkHome = androidSdkDir.map { sdkDir ->
    File(sdkDir, "ndk/$androidNdkVersion").absolutePath
}
// Rust 源码才是真正的输入。若直接用 inputs.dir(rustDir),cargo 每轮写进 target/ 都会
// 让任务失效,且每次构建都要对 GB 级产物目录做指纹计算。
val rustSourceInputs = fileTree(rustDir.asFile) {
    exclude("target/**")
}

android {
    namespace = "com.hambur.chat"
    compileSdk = 37
    ndkVersion = androidNdkVersion

    defaultConfig {
        applicationId = "com.hambur.chat"
        minSdk = 31
        targetSdk = 37
        versionCode = 1
        versionName = "0.1.0-m0"

        ndk {
            abiFilters.add("arm64-v8a")
        }
    }

    sourceSets {
        getByName("main") {
            kotlin.srcDir(generatedUniffiDir.get().asFile)
            jniLibs.srcDir("src/main/jniLibs")
            // 注意: 生成的 Rust 库不再挂在 main 上,否则两个变体会同时包含两份 .so。
        }
        getByName("debug") {
            jniLibs.srcDir(generatedJniLibsDebugDir.get().asFile)
        }
        getByName("release") {
            jniLibs.srcDir(generatedJniLibsReleaseDir.get().asFile)
        }
    }

    buildFeatures {
        compose = true
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    packaging {
        jniLibs {
            useLegacyPackaging = true
            keepDebugSymbols += "*/arm64-v8a/libproot.so"
        }
    }

    buildTypes {
        getByName("release") {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
            signingConfig = signingConfigs.getByName("debug")
        }
    }
}

kotlin {
    compilerOptions {
        jvmTarget.set(JvmTarget.JVM_17)
    }
}

tasks.register<Exec>("installUniFfiBindgen") {
    description = "Installs the UniFFI Kotlin binding generator used by this checkout."
    outputs.file(uniffiBindgenBin)
    commandLine(
        "cargo",
        "install",
        "uniffi",
        "--version",
        uniffiVersion,
        "--bin",
        "uniffi-bindgen",
        "--features",
        "cli",
        "--root",
        uniffiBindgenRoot.get().asFile.absolutePath,
        "--locked"
    )
}

tasks.register<Exec>("generateUniFfiKotlinBindings") {
    description = "Generates Kotlin bindings from the Hambur UniFFI UDL contract."
    dependsOn("installUniFfiBindgen")
    inputs.file(uniffiUdl)
    inputs.file(uniffiConfig)
    outputs.dir(generatedUniffiDir)
    workingDir = rustDir.asFile
    commandLine(
        uniffiBindgenBin.get().absolutePath,
        "generate",
        uniffiUdl.asFile.absolutePath,
        "--language",
        "kotlin",
        "--config",
        uniffiConfig.asFile.absolutePath,
        "--out-dir",
        generatedUniffiDir.get().asFile.absolutePath
    )
}

tasks.register<Exec>("buildRustDebug") {
    description = "Builds the Rust UniFFI cdylib for Android debug packaging."
    inputs.files(rustSourceInputs)
    outputs.dir(generatedJniLibsDebugDir)
    workingDir = rustDir.asFile
    environment("ANDROID_HOME", androidSdkDir.get())
    environment("ANDROID_NDK_HOME", androidNdkHome.get())
    commandLine(
        "cargo",
        "ndk",
        "-t",
        "arm64-v8a",
        "-o",
        generatedJniLibsDebugDir.get().asFile.absolutePath,
        "build",
        "-p",
        "hambur-uniffi"
    )
}

tasks.register<Exec>("buildRustRelease") {
    description = "Builds the Rust UniFFI cdylib for Android release packaging."
    inputs.files(rustSourceInputs)
    outputs.dir(generatedJniLibsReleaseDir)
    workingDir = rustDir.asFile
    environment("ANDROID_HOME", androidSdkDir.get())
    environment("ANDROID_NDK_HOME", androidNdkHome.get())
    commandLine(
        "cargo",
        "ndk",
        "-t",
        "arm64-v8a",
        "-o",
        generatedJniLibsReleaseDir.get().asFile.absolutePath,
        "build",
        "-p",
        "hambur-uniffi",
        "--release"
    )
}

tasks.withType<org.jetbrains.kotlin.gradle.tasks.KotlinCompile>().configureEach {
    dependsOn("generateUniFfiKotlinBindings")
}

// 按构建类型分别挂钩,取代原先"用命令行任务名里是否含 Release 来猜"的做法:
// 那个启发式在 `assembleDebug assembleRelease` 或 `build` 这类同时构建两个变体的
// 调用下只会挂上一个任务,另一个变体就会打包出缺失或错误 flavor 的 .so。
tasks.matching { it.name == "preDebugBuild" }.configureEach {
    dependsOn("buildRustDebug")
}
tasks.matching { it.name == "preReleaseBuild" }.configureEach {
    dependsOn("buildRustRelease")
}

dependencies {
    implementation("androidx.core:core-ktx:1.18.0")
    implementation("androidx.activity:activity-compose:1.13.0")
    implementation("androidx.compose.ui:ui:1.11.2")
    implementation("androidx.compose.ui:ui-tooling-preview:1.11.2")
    implementation("androidx.compose.foundation:foundation:1.11.2")
    implementation("androidx.compose.material3:material3:1.4.0")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.11.0")
    implementation("io.coil-kt.coil3:coil-compose:3.3.0")
    implementation("com.composables:icons-lucide-android:1.1.0")
    implementation("net.java.dev.jna:jna:5.17.0@aar")

    debugImplementation("androidx.compose.ui:ui-tooling:1.11.2")

    testImplementation("junit:junit:4.13.2")
}
