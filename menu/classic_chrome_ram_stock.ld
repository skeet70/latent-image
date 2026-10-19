/* Stock X-M1 1.50 continuation and reader addresses that
 * menu/classic_chrome_ram.S branches to (reports/classic-chrome-patch.md
 * section 8.2.3). Passed to the runtime link as an input script. */
cc_stock_filmmode_return = 0x00eab350;
cc_stock_filmmode_zero = 0x00eab314;
cc_stock_saturation_mode_arm = 0x00eaacc8;
cc_stock_saturation_zero = 0x00eaadc8;
cc_stock_raw_commit_resume = 0x00d461f0;
cc_stock_raw_highlight_done = 0x00d5b344;
cc_stock_raw_highlight_resume = 0x00d5ada8;
cc_stock_raw_request_resume = 0x012548a8;
cc_stock_raw_request_done = 0x01254928;
cc_stock_colour_read = 0x00d670a0;
cc_stock_chroma_read = 0x00d60cc4;
cc_stock_tone_read = 0x00690924;
