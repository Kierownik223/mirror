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
  
});