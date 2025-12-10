var audio = document.getElementById('audio');
var titleEl = document.getElementById('title');
var artistEl = document.getElementById('artist');
var albumEl = document.getElementById('album');
var yearEl = document.getElementById('year');
var genreEl = document.getElementById('genre');
var coverEl = document.getElementById('cover');
var trackEl = document.getElementById('track');
var previous = document.getElementById('previous');
var next = document.getElementById('next');

function fetchJSON(url, callback) {
    var xhr;
    if (window.XMLHttpRequest) {
        xhr = new XMLHttpRequest();
    } else {
        try {
            xhr = new ActiveXObject("Microsoft.XMLHTTP");
        } catch (e) {
            alert("Failed to load resource: " + url);
            return;
        }
    }

    xhr.onreadystatechange = function () {
        if (xhr.readyState === 4) {
            if (xhr.status === 200) {
                try {
                    var data = JSON.parse(xhr.responseText);
                    callback(null, data);
                } catch (e) {
                    callback(e, null);
                }
            } else {
                callback(new Error("Request failed: " + xhr.status), null);
            }
        }
    };
    xhr.open("GET", url, true);
    xhr.send();
}

function updatePageMetadata(meta, newPath, coverFile) {
    if (meta.track) {
        meta.track += '.';
    }
    if (artistEl) artistEl.textContent = meta.artist || 'N/A';
    if (titleEl) titleEl.textContent = meta.title || decodeURIComponent(newPath.split('/').pop());
    if (albumEl) albumEl.textContent = meta.album || 'N/A';
    if (yearEl) yearEl.textContent = meta.year || 'N/A';
    if (genreEl) genreEl.textContent = meta.genre || 'N/A';
    if (trackEl) trackEl.textContent = meta.track || '';

    if (coverEl) {
        if (coverFile) {
            coverEl.src = folderPath + '/' + encodeURIComponent(coverFile);
        } else {
            coverEl.src = '/poster' + newPath;
        }
        coverEl.alt = meta.album || 'N/A';
        coverEl.style.display = '';
    }

    if (window.navigator && navigator.mediaSession) {
        try {
            navigator.mediaSession.metadata = new MediaMetadata({
                title: meta.title,
                artist: meta.artist,
                album: meta.album,
                artwork: [{
                    src: coverFile
                        ? folderPath + '/' + encodeURIComponent(coverFile)
                        : '/poster' + newPath
                }]
            });
        } catch (e) {
            alert(e);
        }
    }
    
    if (meta.title) {
        meta.title += ' - MARMAK Mirror';
    }

    document.title = meta.title || decodeURIComponent(newPath.split('/').pop()) + ' - MARMAK Mirror';

    if (window.history && history.pushState) {
        history.pushState(null, '', newPath);
    }
}

var pathname = window.location.pathname.split("/");
var currentFile = decodeURIComponent(pathname.pop());
var folderPath = decodeURIComponent(pathname.join("/"));
var fileNames = [];
var currentIndex = 0;

fetchJSON('/api/listing' + folderPath, function (err, files) {
    if (err) {
        console.error("Failed to fetch file list", err);
        return;
    }

    var coverFile = null;

    for (var i = 0; i < files.length; i++) {
        var lower = files[i].name.toLowerCase();

        if (lower.match(/\.(mp3|m4a|m4b|flac|wav)$/)) {
            fileNames.push(files[i].name);
        }

        if (lower === "cover.jpg" || lower === "cover.png" || lower === "folder.jpg" || lower === "folder.png") {
            coverFile = files[i].name;
        }
    }

    audio.addEventListener('ended', function () {
        if (currentIndex < fileNames.length - 1) {
            currentIndex++;
            loadTrack(currentIndex);
        }
    });

    currentIndex = fileNames.indexOf(currentFile);
    if (currentIndex === -1) currentIndex = 0;

    function loadTrack(index) {
        if (index < 0 || index >= fileNames.length) return;

        var targetFile = fileNames[index];
        var newPath = folderPath + '/' + encodeURIComponent(targetFile);

        audio.src = '/file' + newPath;
        audio.play();

        fetchJSON('/api' + newPath, function (err, meta) {
            if (!err && meta) {
                updatePageMetadata(meta, newPath, coverFile);
            }
        });
    }

    previous.onclick = function () {
        if (currentIndex > 0) {
            currentIndex--;
            loadTrack(currentIndex);
        }
    };

    next.onclick = function () {
        if (currentIndex < fileNames.length - 1) {
            currentIndex++;
            loadTrack(currentIndex);
        }
    };

    if (navigator.mediaSession) {
        try {
            navigator.mediaSession.setActionHandler('previoustrack', function () {
                if (currentIndex > 0) {
                    currentIndex--;
                    loadTrack(currentIndex);
                }
            });

            navigator.mediaSession.setActionHandler('nexttrack', function () {
                if (currentIndex < fileNames.length - 1) {
                    currentIndex++;
                    loadTrack(currentIndex);
                }
            });
        } catch (e) {
            alert(e);
        }
    }
});