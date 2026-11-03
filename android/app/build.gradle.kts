import java.util.Properties
import javax.inject.Inject

plugins {
    id("com.android.application")
}

val repo: File = rootDir.parentFile
val cargoVersion = Regex("""(?m)^version = "([^"]+)"""")
    .find(File(repo, "Cargo.toml").readText())!!
    .groupValues[1]
val (major, minor, patch) = cargoVersion.split(".", "-").take(3).map { it.toInt() }

val keystoreFile: File = rootProject.file("keystore.properties")
val keystore = Properties().apply { if (keystoreFile.isFile) keystoreFile.inputStream().use(::load) }
fun releaseKey(env: String, property: String): String? =
    providers.environmentVariable(env).orNull?.takeIf { it.isNotEmpty() } ?: keystore.getProperty(property)

abstract class CargoNdk @Inject constructor(private val exec: ExecOperations) : DefaultTask() {
    @get:Input
    abstract val abis: ListProperty<String>

    @get:Input
    abstract val profile: Property<String>

    @get:Internal
    abstract val workspace: DirectoryProperty

    @get:Internal
    abstract val ndk: DirectoryProperty

    @get:OutputDirectory
    abstract val outputDir: DirectoryProperty

    @TaskAction
    fun build() {
        val triples = mapOf(
            "arm64-v8a" to "aarch64-linux-android",
            "x86_64" to "x86_64-linux-android",
        )
        val root = workspace.get().asFile
        val target = System.getenv("CARGO_TARGET_DIR")?.let(::File) ?: File(root, "target")
        val profileDir = if (profile.get() == "dev") "debug" else profile.get()
        val out = outputDir.get().asFile
        out.deleteRecursively()
        for (abi in abis.get()) {
            exec.exec {
                workingDir = root
                environment("ANDROID_NDK_HOME", ndk.get().asFile.absolutePath)
                commandLine(
                    "cargo", "ndk", "--target", abi, "--platform", "26",
                    "rustc", "--package", "citrine-gb-app", "--lib",
                    "--crate-type", "cdylib", "--profile", profile.get(),
                )
            }
            File(target, "${triples.getValue(abi)}/$profileDir/libcitrine_gb_app.so")
                .copyTo(File(out, "$abi/libcitrine_gb_app.so"), overwrite = true)
        }
    }
}

abstract class LauncherIcon : DefaultTask() {
    @get:InputFile
    abstract val icon: RegularFileProperty

    @get:OutputDirectory
    abstract val outputDir: DirectoryProperty

    @TaskAction
    fun copy() {
        icon.get().asFile.copyTo(
            File(outputDir.get().asFile, "mipmap-xxxhdpi/ic_launcher.png"),
            overwrite = true,
        )
    }
}

android {
    namespace = "io.github.zitronenjoghurt.citrine"
    compileSdk = 36
    ndkVersion = "29.0.14206865"

    defaultConfig {
        applicationId = "io.github.zitronenjoghurt.citrine"
        minSdk = 26
        targetSdk = 36
        versionCode = major * 10000 + minor * 100 + patch
        versionName = cargoVersion
    }

    testOptions {
        unitTests.isReturnDefaultValues = true
    }

    signingConfigs {
        getByName("debug") {
            storeFile = rootProject.file("debug.keystore")
            storePassword = "android"
            keyAlias = "androiddebugkey"
            keyPassword = "android"
        }
        val store = releaseKey("CITRINE_KEYSTORE", "storeFile")
        if (store != null) {
            create("release") {
                storeFile = rootProject.file(store)
                storePassword = releaseKey("CITRINE_KEYSTORE_PASSWORD", "storePassword")
                keyAlias = releaseKey("CITRINE_KEY_ALIAS", "keyAlias")
                keyPassword = releaseKey("CITRINE_KEY_PASSWORD", "keyPassword")
            }
        }
    }

    buildTypes {
        getByName("release") {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            signingConfig = signingConfigs.findByName("release")
        }
    }
}

val launcherIcon = tasks.register<LauncherIcon>("launcherIcon") {
    icon.set(File(repo, "crates/citrine-gb-app/assets/icon-192x192.png"))
    outputDir.set(layout.buildDirectory.dir("rust/res"))
}

androidComponents {
    onVariants { variant ->
        val name = variant.name.replaceFirstChar { it.uppercase() }
        val defaultProfile = if (variant.buildType == "release") "dist-android" else "release"
        val cargoNdk = tasks.register<CargoNdk>("cargoNdk$name") {
            abis.set(providers.gradleProperty("citrine.abis").orElse("arm64-v8a").map { it.split(",") })
            profile.set(providers.gradleProperty("citrine.rustProfile").orElse(defaultProfile))
            workspace.set(repo)
            ndk.set(androidComponents.sdkComponents.ndkDirectory)
            outputDir.set(layout.buildDirectory.dir("rust/${variant.name}/jniLibs"))
            outputs.upToDateWhen { false }
        }
        variant.sources.jniLibs?.addGeneratedSourceDirectory(cargoNdk, CargoNdk::outputDir)
        variant.packaging.jniLibs.keepDebugSymbols.add("**/libcitrine_gb_app.so")
        variant.sources.res?.addGeneratedSourceDirectory(launcherIcon, LauncherIcon::outputDir)
    }
}

dependencies {
    implementation("androidx.games:games-activity:2.0.2")
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.activity:activity:1.10.1")
    implementation("androidx.core:core:1.16.0")
    implementation("androidx.documentfile:documentfile:1.1.0")
    testImplementation("junit:junit:4.13.2")
}
