import { AnimatePresence, motion } from "motion/react";
import type { Settings } from "../../api";
import { publicBlock, useNetwork } from "../../data";
import { Icon } from "../../ui/Icon";
import { RISE, SPRING, TWEEN } from "../../ui/motion";
import { t } from "../../i18n";

/** Whether launching is blocked here: a Public network with the guard on (`network`). */
export const usePublicBlock = (settings: Settings | undefined) => publicBlock(useNetwork().data, settings) !== null;

/** While the vault is locked (no island), the public-network block as a slim pill under the title
 * bar. It drops in from where the island would be, and lifts away faster. */
export function PublicNetworkBanner({ settings }: { settings: Settings }) {
  const blocked = usePublicBlock(settings);
  // The live region stays put (out of the layout); only its words come and go, so they are read.
  return (
    <div role="status" className="contents">
      <AnimatePresence>
        {blocked && (
          <motion.div
            key="public"
            className="set-public-pill"
            initial={{ opacity: 0, y: -RISE.md }}
            animate={{ opacity: 1, y: 0, transition: SPRING.smooth }}
            exit={{ opacity: 0, y: -RISE.sm, transition: TWEEN.exit }}
          >
            <Icon name="globe" size={14} />
            <span>{t("settings.publicBannerBlocked")}</span>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
