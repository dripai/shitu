package com.dripai.shiping.recording

import android.content.ContentUris
import android.content.Context
import android.media.MediaMetadataRetriever
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.MediaStore
import androidx.core.content.FileProvider
import com.dripai.shiping.BuildConfig
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.io.File

data class RecordingItem(
    val uri: Uri,
    val displayName: String,
    val durationMs: Long,
    val sizeBytes: Long,
    val createdAtMs: Long,
)

class RecordingHistoryRepository(
    private val context: Context,
) {
    suspend fun load(): List<RecordingItem> = withContext(Dispatchers.IO) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            loadFromMediaStore()
        } else {
            loadFromLegacyDirectory()
        }
    }

    private fun loadFromMediaStore(): List<RecordingItem> {
        val collection = MediaStore.Video.Media.getContentUri(
            MediaStore.VOLUME_EXTERNAL_PRIMARY,
        )
        val projection = arrayOf(
            MediaStore.Video.Media._ID,
            MediaStore.Video.Media.DISPLAY_NAME,
            MediaStore.Video.Media.DURATION,
            MediaStore.Video.Media.SIZE,
            MediaStore.Video.Media.DATE_ADDED,
        )
        val directory = "${Environment.DIRECTORY_MOVIES}/ShiPing/"
        val recordings = mutableListOf<RecordingItem>()

        context.contentResolver.query(
            collection,
            projection,
            "${MediaStore.Video.Media.RELATIVE_PATH} = ?",
            arrayOf(directory),
            "${MediaStore.Video.Media.DATE_ADDED} DESC",
        )?.use { cursor ->
            val idColumn = cursor.getColumnIndexOrThrow(MediaStore.Video.Media._ID)
            val nameColumn = cursor.getColumnIndexOrThrow(MediaStore.Video.Media.DISPLAY_NAME)
            val durationColumn = cursor.getColumnIndexOrThrow(MediaStore.Video.Media.DURATION)
            val sizeColumn = cursor.getColumnIndexOrThrow(MediaStore.Video.Media.SIZE)
            val dateColumn = cursor.getColumnIndexOrThrow(MediaStore.Video.Media.DATE_ADDED)

            while (cursor.moveToNext()) {
                recordings += RecordingItem(
                    uri = ContentUris.withAppendedId(collection, cursor.getLong(idColumn)),
                    displayName = cursor.getString(nameColumn) ?: "ShiPing 录制",
                    durationMs = cursor.getLong(durationColumn).coerceAtLeast(0),
                    sizeBytes = cursor.getLong(sizeColumn).coerceAtLeast(0),
                    createdAtMs = cursor.getLong(dateColumn).coerceAtLeast(0) * 1_000,
                )
            }
        }
        return recordings
    }

    private fun loadFromLegacyDirectory(): List<RecordingItem> {
        val root = context.getExternalFilesDir(Environment.DIRECTORY_MOVIES)
            ?: return emptyList()
        val directory = File(root, "ShiPing")
        val files = directory.listFiles { file ->
            file.isFile && file.extension.equals("mp4", ignoreCase = true)
        }.orEmpty()

        return files
            .sortedByDescending(File::lastModified)
            .map { file ->
                RecordingItem(
                    uri = FileProvider.getUriForFile(
                        context,
                        "${BuildConfig.APPLICATION_ID}.files",
                        file,
                    ),
                    displayName = file.name,
                    durationMs = readDuration(file),
                    sizeBytes = file.length(),
                    createdAtMs = file.lastModified(),
                )
            }
    }

    private fun readDuration(file: File): Long {
        val retriever = MediaMetadataRetriever()
        return try {
            retriever.setDataSource(file.absolutePath)
            retriever.extractMetadata(MediaMetadataRetriever.METADATA_KEY_DURATION)
                ?.toLongOrNull()
                ?.coerceAtLeast(0)
                ?: 0
        } catch (_: RuntimeException) {
            0
        } finally {
            retriever.release()
        }
    }
}
