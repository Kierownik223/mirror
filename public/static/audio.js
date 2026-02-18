var audio = document.getElementById("audio");
var titleEl = document.getElementById("title");
var artistEl = document.getElementById("artist");
var albumEl = document.getElementById("album");
var yearEl = document.getElementById("year");
var genreEl = document.getElementById("genre");
var coverEl = document.getElementById("cover");
var trackEl = document.getElementById("track");
var downloadEl = document.getElementById("download");
var breadcrumbsEl = document.getElementsByClassName("breadcrumbs")[0];
var previous = document.getElementById("previous");
var next = document.getElementById("next");
var autoplay = document.getElementById("autoplay");

var cookies = document.cookie.split(';');
for (var i = 0; i < cookies.length; i++) {
    var cookie = cookies[i].replace(/^\s+/, '');
    if (cookie.indexOf("audiovolume=") === 0) {
        audio.volume = cookie.substring("audiovolume=".length);
    }
}

audio.addEventListener("volumechange", function () {
    document.cookie = "audiovolume=" + audio.volume + "; path=/"; 
});

(function () {
    function getQueryParam(name) {
        var match = new RegExp('[?&]' + name + '=([^&]*)').exec(window.location.search);
        return match && decodeURIComponent(match[1].replace(/\+/g, ' '));
    }

    function parseTimeToSeconds(timeStr) {
        if (!timeStr) return null;

        timeStr = timeStr.toLowerCase().trim();

        if (/^\d+$/.test(timeStr)) {
            return parseInt(timeStr, 10);
        }

        var total = 0;
        var match;

        var regex = /(\d+)(h|m|s)/g;
        while ((match = regex.exec(timeStr)) !== null) {
            var value = parseInt(match[1], 10);
            var unit = match[2];

            if (unit === "h") total += value * 3600;
            if (unit === "m") total += value * 60;
            if (unit === "s") total += value;
        }

        if (total > 0) return total;

        if (timeStr.indexOf(":") !== -1) {
            var parts = timeStr.split(":");
            var seconds = 0;

            if (parts.length === 2) {
                seconds =
                parseInt(parts[0], 10) * 60 +
                parseInt(parts[1], 10);
            } else if (parts.length === 3) {
                seconds =
                parseInt(parts[0], 10) * 3600 +
                parseInt(parts[1], 10) * 60 +
                parseInt(parts[2], 10);
            }

            return seconds;
        }

        return null;
    }
    var timeParam = getQueryParam("t");
    var startTime = parseTimeToSeconds(timeParam);

    if (startTime !== null && !isNaN(startTime)) {
        var onLoadedData = function () {
            audio.currentTime = Math.min(
                Math.max(0, startTime),
                audio.duration
            );

            audio.removeEventListener("loadeddata", onLoadedData);
        };

        audio.addEventListener("loadeddata", onLoadedData);
    }
})();

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

function updatePageMetadata(meta, newPath, coverFile, push) {
    if (meta.track) {
        meta.track += ".";
    }
    if (artistEl) artistEl.textContent = meta.artist || "N/A";
    if (titleEl)
        titleEl.textContent =
            meta.title || decodeURIComponent(newPath.split("/").pop());
    if (albumEl) albumEl.textContent = meta.album || "N/A";
    if (yearEl) yearEl.textContent = meta.year || "N/A";
    if (genreEl) genreEl.textContent = meta.genre || "N/A";
    if (trackEl) trackEl.textContent = meta.track || "";
    if (downloadEl) downloadEl.href = newPath + "?download";
    if (breadcrumbsEl)
        breadcrumbsEl.outerHTML =
            createBreadcrumbs(decodeURIComponent(newPath)).outerHTML ||
            breadcrumbsEl;
    breadcrumbsEl = document.getElementsByClassName("breadcrumbs")[0];

    if (coverEl) {
        if (coverFile) {
            coverEl.src = folderPath + "/" + encodeURIComponent(coverFile);
        } else {
            coverEl.src = "/poster" + newPath;
        }
        coverEl.alt = meta.album || "N/A";
        coverEl.style.display = "";
    }

    if (window.navigator && navigator.mediaSession) {
        try {
            navigator.mediaSession.metadata = new MediaMetadata({
                title: meta.title,
                artist: meta.artist,
                album: meta.album,
                artwork: [
                    {
                        src: coverFile
                            ? folderPath + "/" + encodeURIComponent(coverFile)
                            : "/poster" + newPath,
                    },
                ],
            });
        } catch (e) {
            alert(e);
        }
    }

    if (meta.title) {
        meta.title += " - MARMAK Mirror";
    }

    document.title =
        meta.title ||
        decodeURIComponent(newPath.split("/").pop()) + " - MARMAK Mirror";

    if (push && window.history && history.pushState) {
        history.pushState(null, "", newPath);
    }
}

var pathname = window.location.pathname.split("/");
var currentFile = decodeURIComponent(pathname.pop());
var folderPath = decodeURIComponent(pathname.join("/"));
var fileNames = [];
var currentIndex = 0;

fetchJSON("/api/listing" + folderPath, function (err, files) {
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

        if (
            lower === "cover.jpg" ||
            lower === "cover.png" ||
            lower === "folder.jpg" ||
            lower === "folder.png"
        ) {
            coverFile = files[i].name;
        }
    }

    audio.addEventListener("ended", function () {
        if (autoplay.checked) {
            if (currentIndex + 1 >= fileNames.length - 1) {
                next.style.display = "none";
            } else {
                next.style.display = "inline";
            }
            if (currentIndex + 1 == 0 || fileNames.length == 1) {
                previous.style.display = "none";
            } else {
                previous.style.display = "inline";
            }
            if (currentIndex < fileNames.length - 1) {
                currentIndex++;
                loadTrack(currentIndex);
            }
        }
    });

    currentIndex = fileNames.indexOf(currentFile);
    if (currentIndex === -1) currentIndex = 0;

    if (currentIndex == fileNames.length - 1) {
        next.style.display = "none";
    } else {
        next.style.display = "inline";
    }
    if (currentIndex == 0) {
        previous.style.display = "none";
    } else {
        previous.style.display = "inline";
    }

    function loadTrack(index) {
        if (index < 0 || index >= fileNames.length) return;

        var targetFile = fileNames[index];
        var newPath = folderPath + "/" + encodeURIComponent(targetFile);

        audio.src = newPath + "?download";
        audio.play();

        fetchJSON("/api" + newPath, function (err, meta) {
            if (!err && meta) {
                updatePageMetadata(meta, newPath, coverFile, true);
            }
        });
    }

    window.addEventListener("popstate", function () {
        var pathname = window.location.pathname.split("/");
        var file = decodeURIComponent(pathname.pop());

        var index = fileNames.indexOf(file);
        if (index !== -1) {
            currentIndex = index;

            next.style.display = currentIndex >= fileNames.length - 1 ? "none" : "inline";
            previous.style.display = currentIndex <= 0 ? "none" : "inline";

            var targetFile = fileNames[currentIndex];
            var newPath = folderPath + "/" + encodeURIComponent(targetFile);

            audio.src = newPath + "?download";
            audio.play();

            fetchJSON("/api" + newPath, function (err, meta) {
                if (!err && meta) {
                    updatePageMetadata(meta, newPath, coverFile, false);
                }
            });
        }
    });

    previous.onclick = function () {
        if (currentIndex - 1 == fileNames.length) {
            next.style.display = "none";
        } else {
            next.style.display = "inline";
        }
        if (currentIndex - 1 == 0) {
            previous.style.display = "none";
        } else {
            previous.style.display = "inline";
        }
        if (currentIndex > 0) {
            currentIndex--;
            loadTrack(currentIndex);
        }
    };

    next.onclick = function () {
        if (currentIndex + 1 == fileNames.length - 1) {
            next.style.display = "none";
        } else {
            next.style.display = "inline";
        }
        if (currentIndex + 1 == 0) {
            previous.style.display = "none";
        } else {
            previous.style.display = "inline";
        }
        if (currentIndex < fileNames.length - 1) {
            currentIndex++;
            loadTrack(currentIndex);
        }
    };

    if (navigator.mediaSession) {
        try {
            navigator.mediaSession.setActionHandler(
                "previoustrack",
                function () {
                    if (currentIndex - 1 == fileNames.length) {
                        next.style.display = "none";
                    } else {
                        next.style.display = "inline";
                    }
                    if (currentIndex - 1 == 0) {
                        previous.style.display = "none";
                    } else {
                        previous.style.display = "inline";
                    }
                    if (currentIndex > 0) {
                        currentIndex--;
                        loadTrack(currentIndex);
                    }
                }
            );

            navigator.mediaSession.setActionHandler("nexttrack", function () {
                if (currentIndex + 1 == fileNames.length - 1) {
                    next.style.display = "none";
                } else {
                    next.style.display = "inline";
                }
                if (currentIndex + 1 == 0) {
                    previous.style.display = "none";
                } else {
                    previous.style.display = "inline";
                }
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
