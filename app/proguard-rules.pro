# JNA rules
-keep class net.java.dev.jna.** { *; }
-dontwarn net.java.dev.jna.**
-keep class com.sun.jna.** { *; }
-dontwarn com.sun.jna.**

# Keep UniFFI-generated interfaces and implementation classes
-keep class com.hambur.chat.** { *; }
-keep class uniffi.** { *; }

# Keep all native method names
-keepclasseswithmembernames class * {
    native <methods>;
}

# Android Compose and other standard rules are automatically handled by default proguard files,
# but we can add keep rules for any other reflection-based libraries if needed.
