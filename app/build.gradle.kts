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
val generatedJniLibsDir = layout.buildDirectory.dir("generated/rustJniLibs")
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
            jniLibs.srcDir(generatedJniLibsDir.get().asFile)
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

    signingConfigs {
        getByName("debug") {
            storeFile = file("debug.keystore")
            storePassword = "android"
            keyAlias = "androiddebugkey"
            keyPassword = "android"
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
    inputs.dir(rustDir)
    outputs.dir(generatedJniLibsDir)
    workingDir = rustDir.asFile
    environment("ANDROID_HOME", androidSdkDir.get())
    environment("ANDROID_NDK_HOME", androidNdkHome.get())
    commandLine(
        "cargo",
        "ndk",
        "-t",
        "arm64-v8a",
        "-o",
        generatedJniLibsDir.get().asFile.absolutePath,
        "build",
        "-p",
        "hambur-uniffi"
    )
}

tasks.register<Exec>("buildRustRelease") {
    description = "Builds the Rust UniFFI cdylib for Android release packaging."
    inputs.dir(rustDir)
    outputs.dir(generatedJniLibsDir)
    workingDir = rustDir.asFile
    environment("ANDROID_HOME", androidSdkDir.get())
    environment("ANDROID_NDK_HOME", androidNdkHome.get())
    commandLine(
        "cargo",
        "ndk",
        "-t",
        "arm64-v8a",
        "-o",
        generatedJniLibsDir.get().asFile.absolutePath,
        "build",
        "-p",
        "hambur-uniffi",
        "--release"
    )
}

tasks.withType<org.jetbrains.kotlin.gradle.tasks.KotlinCompile>().configureEach {
    dependsOn("generateUniFfiKotlinBindings")
}

val isRelease = gradle.startParameter.taskNames.any { it.contains("Release", ignoreCase = true) }
tasks.named("preBuild") {
    if (isRelease) {
        dependsOn("buildRustRelease")
    } else {
        dependsOn("buildRustDebug")
    }
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
    implementation("io.coil-kt.coil3:coil-network-okhttp:3.3.0")
    implementation("com.composables:icons-lucide-android:1.1.0")
    implementation("net.java.dev.jna:jna:5.17.0@aar")

    debugImplementation("androidx.compose.ui:ui-tooling:1.11.2")

    testImplementation("junit:junit:4.13.2")
}
