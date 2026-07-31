package com.dripai.shiping.ui.theme

import android.os.Build
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext

private val DarkColors = darkColorScheme(
    primary = Color(0xFFFF5576),
    onPrimary = Color.White,
    primaryContainer = Color(0xFF652033),
    secondary = Color(0xFF79D9E8),
    background = Color(0xFF10131A),
    surface = Color(0xFF171C25),
    surfaceVariant = Color(0xFF202735),
)

private val LightColors = lightColorScheme(
    primary = Color(0xFFC9244B),
    onPrimary = Color.White,
    primaryContainer = Color(0xFFFFD9E0),
    secondary = Color(0xFF006879),
    background = Color(0xFFFFF8F8),
    surface = Color.White,
    surfaceVariant = Color(0xFFF3EDEF),
)

@Composable
fun ShiPingTheme(
    darkTheme: Boolean = isSystemInDarkTheme(),
    dynamicColor: Boolean = false,
    content: @Composable () -> Unit,
) {
    val colors = when {
        dynamicColor && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S -> {
            val context = LocalContext.current
            if (darkTheme) dynamicDarkColorScheme(context) else dynamicLightColorScheme(context)
        }
        darkTheme -> DarkColors
        else -> LightColors
    }

    MaterialTheme(
        colorScheme = colors,
        typography = androidx.compose.material3.Typography(),
        content = content,
    )
}
