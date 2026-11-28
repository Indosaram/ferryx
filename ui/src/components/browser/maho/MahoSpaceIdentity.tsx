import { BookOpen, Briefcase, Code, Coffee, Globe, GraduationCap, Heart, House, Music, Palette, Rocket, Star } from "lucide-react";
import { isSpaceIdentityIcon } from "../../../state/browserStore";

const ICONS = { BookOpen, Briefcase, Code, Coffee, Globe, GraduationCap, Heart, House, Music, Palette, Rocket, Star };

export function MahoSpaceIdentity({ icon }: { icon?: string }) {
  if (!isSpaceIdentityIcon(icon)) return null;
  const Icon = ICONS[icon];
  return <Icon aria-hidden="true" className="maho-space-identity" />;
}
