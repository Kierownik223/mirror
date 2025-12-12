document.addEventListener("DOMContentLoaded", () => {
    const video = document.createElement("video");
    video.src = "/static/images/greatest/powerofgreatness.mp4?download";
    video.autoplay = true;
    video.muted = true;
    video.loop = true;
    video.playsInline = true;
    video.style.position = "fixed";
    video.style.top = "50%";
    video.style.left = "50%";
    video.style.width = "100%";
    video.style.height = "100%";
    video.style.objectFit = "cover";
    video.style.transform = "translate(-50%, -50%)";
    video.style.zIndex = "-1";
    video.style.pointerEvents = "none";
    document.body.appendChild(video);

    const soundextensions = [".mp3", ".wav", ".ogg", ".flac", ".m4a",".mp4",".mkv",".webm"];
    const url = window.location.href;
    const hassoundfile = soundextensions.some(ext => url.includes(ext));

    if (!hassoundfile) {
        const audio = document.createElement("audio");
        audio.src = "/static/images/greatest/greatness.mp3?download";
        audio.loop = true;
        audio.volume = 0.5;
        audio.style.display = "none";
        document.body.appendChild(audio);

        const playaudio = () => {
            audio.play().catch(() => { });
            document.removeEventListener("click", playaudio);
            document.removeEventListener("keydown", playaudio);
            document.removeEventListener("touchstart", playaudio);
        };

        document.addEventListener("click", playaudio);
        document.addEventListener("keydown", playaudio);
        document.addEventListener("touchstart", playaudio);
    }
});