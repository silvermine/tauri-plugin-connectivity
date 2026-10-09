import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

// Standalone `android/settings.gradle` includes `:lib`; a Tauri host app does not.
val isStandaloneLibBuild = findProject(":lib")?.projectDir == file("lib")

android {
    namespace = "org.silvermine.plugin.connectivity"
    compileSdk = 37

    defaultConfig {
        minSdk = 23

        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        consumerProguardFiles("consumer-rules.pro")
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    if (!isStandaloneLibBuild) {
        sourceSets {
            named("main") {
                java.srcDir("lib/src/main/java")
            }
        }
    }
}

kotlin {
    compilerOptions {
        jvmTarget = JvmTarget.JVM_17
    }
}

dependencies {
    if (isStandaloneLibBuild) {
        implementation(project(":lib"))
    }
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.1.5")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.5.1")
    if (findProject(":tauri-android") != null) {
        implementation(project(":tauri-android"))
    }
}
