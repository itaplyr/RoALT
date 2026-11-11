// Dev only: the gallery with what the app's root provides (motion settings, toasts, tooltips).
import { Announcer } from "../ui/a11y";
import { MotionRoot } from "../ui/motion";
import { Toaster } from "../ui/Toast";
import { TooltipHost } from "../ui/Tooltip";
import { Gallery } from "./Gallery";

export function GalleryRoot() {
  return (
    <MotionRoot>
      <Gallery />
      <Toaster />
      <TooltipHost />
      <Announcer />
    </MotionRoot>
  );
}
