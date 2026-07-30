package com.dripai.shiping

import android.app.Activity
import android.app.NativeActivity
import android.content.Intent
import android.media.projection.MediaProjectionManager
import android.os.Bundle

class MainActivity : NativeActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
    }

    fun requestScreenCapturePermission() {
        runOnUiThread {
            val projectionManager = getSystemService(MediaProjectionManager::class.java)
            startActivityForResult(
                projectionManager.createScreenCaptureIntent(),
                SCREEN_CAPTURE_REQUEST_CODE,
            )
        }
    }

    @Deprecated("Required by the NativeActivity screen-capture permission bridge")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode != SCREEN_CAPTURE_REQUEST_CODE) {
            return
        }

        nativeOnScreenCapturePermissionResult(
            resultCode == Activity.RESULT_OK && data != null,
        )
    }

    private external fun nativeOnScreenCapturePermissionResult(granted: Boolean)

    private companion object {
        const val SCREEN_CAPTURE_REQUEST_CODE = 4101
    }
}
